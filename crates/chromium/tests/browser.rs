//! Real-browser integration tests against local fixture pages.
//! Set SWIFTBROWSE_SKIP_BROWSER_TESTS=1 to skip them where no Chromium exists;
//! otherwise a missing browser is a test failure, not a silent pass.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

use serde_json::{json, Value};
use swiftbrowse_chromium::{launch, ChromiumFactory};
use swiftbrowse_core::result::VerificationStatus;
use swiftbrowse_core::{ActionResult, ErrorKind, Policy, Request, SessionManager, SessionOptions};

fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let target = req.split_whitespace().nth(1).unwrap_or("/");
                let path = target.split('?').next().unwrap_or("/");
                let (status, body) = match path {
                    "/form" => ("200 OK", include_str!("fixtures/form.html")),
                    "/tricky" => ("200 OK", include_str!("fixtures/tricky.html")),
                    _ => ("404 Not Found", "not found"),
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

struct Harness {
    mgr: SessionManager,
    id: String,
    base: String,
    _evidence: tempfile::TempDir,
}

async fn harness() -> Option<Harness> {
    if launch::discover().is_none() {
        if std::env::var("SWIFTBROWSE_SKIP_BROWSER_TESTS").is_ok_and(|v| v == "1") {
            eprintln!("skipping: no Chromium found");
            return None;
        }
        panic!("no Chromium found; set SWIFTBROWSE_CHROMIUM or SWIFTBROWSE_SKIP_BROWSER_TESTS=1");
    }
    let evidence = tempfile::tempdir().unwrap();
    let policy = Policy { evidence_dir: evidence.path().to_path_buf(), default_timeout_ms: 3_000, ..Policy::default() };
    let mut mgr = SessionManager::new(Arc::new(ChromiumFactory::default()), policy);
    let id = mgr.create(None, &SessionOptions::default()).await.unwrap().session_id;
    Some(Harness { mgr, id, base: serve(), _evidence: evidence })
}

impl Harness {
    async fn run(&mut self, op: &str, args: Value) -> ActionResult {
        self.mgr.execute(&self.id, Request::from_op(op, args).unwrap()).await
    }
    async fn ok(&mut self, op: &str, args: Value) -> ActionResult {
        let r = self.run(op, args).await;
        assert!(r.ok, "{op} failed: {}", serde_json::to_string_pretty(&r).unwrap());
        r
    }
    async fn open(&mut self, path: &str) -> ActionResult {
        let url = format!("{}{path}", self.base);
        self.ok("navigate", json!({ "url": url })).await
    }
}

fn kind(r: &ActionResult) -> ErrorKind {
    r.error.as_ref().unwrap_or_else(|| panic!("expected error: {r:?}")).kind
}

#[tokio::test(flavor = "multi_thread")]
async fn form_flow_is_verified_end_to_end() {
    let Some(mut h) = harness().await else { return };
    let nav = h.open("/form").await;
    assert_eq!(nav.data.as_ref().unwrap()["status"], 200);
    assert_eq!(nav.page.as_ref().unwrap().title, "Sign up");

    let snap = h.ok("get_page_snapshot", json!({})).await.data.unwrap();
    assert_eq!(snap["headings"][0]["text"], "Create account");
    let names: Vec<&str> = snap["elements"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    for want in ["Email", "Password", "Country", "I agree to the terms", "Birthday", "Bio", "Sign up"] {
        assert!(names.contains(&want), "snapshot missing {want}: {names:?}");
    }

    h.ok("fill", json!({ "target": { "by": "label", "text": "Email" }, "value": "ada@example.com" })).await;
    let pw = h.ok("fill", json!({ "target": { "by": "label", "text": "Password" }, "value": "s3cret!" })).await;
    let pw_json = serde_json::to_string(&pw).unwrap();
    assert!(!pw_json.contains("s3cret"), "password leaked: {pw_json}");
    h.ok("fill", json!({ "target": { "by": "auto", "query": "Birthday" }, "value": "1990-12-10" })).await;
    h.ok("fill", json!({ "target": { "by": "role", "role": "textbox", "name": "Bio" }, "value": "Hello\nworld" }))
        .await;
    let sel =
        h.ok("select_option", json!({ "target": { "by": "label", "text": "Country" }, "option": "Netherlands" })).await;
    assert_eq!(sel.data.unwrap()["selected"][0], "nl");

    // Submitting without accepting the terms shows a validation alert, not success.
    let r = h
        .run(
            "click",
            json!({ "target": { "by": "role", "role": "button", "name": "Sign up" },
            "expect": { "type": "text_visible", "text": "Welcome" }, "timeout_ms": 600 }),
        )
        .await;
    assert_eq!(kind(&r), ErrorKind::VerificationFailed);
    assert!(!r.evidence.is_empty(), "failure should capture a screenshot");
    assert!(std::path::Path::new(&r.evidence[0].path).exists());
    let snap = h.ok("get_page_snapshot", json!({})).await.data.unwrap();
    assert!(snap["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["text"].as_str().unwrap().contains("accept the terms")));

    h.ok("click", json!({ "target": { "by": "label", "text": "I agree" } })).await;
    h.ok("assert_page_state", json!({ "conditions": [{ "type": "checked", "target": { "by": "role", "role": "checkbox" }, "checked": true }] })).await;
    let r = h
        .ok(
            "click",
            json!({ "target": { "by": "auto", "query": "Sign up" },
            "expect": { "type": "text_visible", "text": "Welcome, ada@example.com from nl" } }),
        )
        .await;
    assert_eq!(r.verification.unwrap().status, VerificationStatus::Passed);
    h.ok("assert_page_state", json!({ "conditions": [{ "type": "url_contains", "value": "/welcome" }] })).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ambiguity_actionability_and_waiting() {
    let Some(mut h) = harness().await else { return };
    h.open("/tricky").await;

    let r = h.run("click", json!({ "target": { "by": "role", "role": "button", "name": "Delete" } })).await;
    assert_eq!(kind(&r), ErrorKind::Ambiguous);
    assert_eq!(r.error.unwrap().details["total"], 2);

    // Overlay covers the page for 400 ms: the click waits instead of clicking the overlay.
    h.ok("click", json!({ "target": { "by": "auto", "query": "Show overlay" } })).await;
    let r = h
        .ok(
            "click",
            json!({ "target": { "by": "auto", "query": "Under overlay" },
            "expect": { "type": "title_contains", "value": "covered clicked" } }),
        )
        .await;
    assert!(r.notes.iter().any(|n| n.contains("covered by")), "{:?}", r.notes);

    // Element that does not exist yet.
    h.ok(
        "click",
        json!({ "target": { "by": "text", "text": "Late button", "exact": true },
        "expect": { "type": "text_visible", "text": "Late clicked" } }),
    )
    .await;

    // Hidden duplicate is ignored; the visible Menu opens the menu.
    h.ok(
        "click",
        json!({ "target": { "by": "role", "role": "button", "name": "Menu" },
        "expect": { "type": "visible", "target": { "by": "role", "role": "link", "name": "Account settings" } } }),
    )
    .await;

    // Open shadow DOM.
    h.ok(
        "click",
        json!({ "target": { "by": "role", "role": "button", "name": "Shadow action" },
        "expect": { "type": "text_visible", "text": "Shadow done" } }),
    )
    .await;

    // A button still sliding into place is not clicked mid-animation.
    h.ok("click", json!({ "target": { "by": "auto", "query": "Start slide" } })).await;
    let r = h
        .ok(
            "click",
            json!({ "target": { "by": "role", "role": "button", "name": "Slider" },
            "expect": { "type": "text_visible", "text": "Slid" } }),
        )
        .await;
    assert!(r.notes.iter().any(|n| n.contains("still moving")), "{:?}", r.notes);

    // Delete row 2 and confirm the count changed.
    h.ok(
        "click",
        json!({ "target": { "by": "role", "role": "button", "name": "Delete", "nth": 1 }, "approved": true,
        "expect": { "type": "count", "target": { "by": "role", "role": "button", "name": "Delete" }, "count": 1 } }),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_dialogs_stale_refs_and_diagnostics() {
    let Some(mut h) = harness().await else { return };
    h.open("/tricky").await;

    let r = h.run("click", json!({ "target": { "by": "test_id", "id": "checkout" } })).await;
    assert_eq!(kind(&r), ErrorKind::ApprovalRequired);
    h.ok("assert_page_state", json!({ "conditions": [{ "type": "text_absent", "text": "Order placed" }] })).await;
    h.ok(
        "click",
        json!({ "target": { "by": "test_id", "id": "checkout" }, "approved": true,
        "expect": { "type": "text_visible", "text": "Order placed" } }),
    )
    .await;

    // confirm() is dismissed, never accepted on the user's behalf.
    h.ok(
        "click",
        json!({ "target": { "by": "auto", "query": "Ask" },
        "expect": { "type": "text_visible", "text": "no" } }),
    )
    .await;
    let snap = h.ok("get_page_snapshot", json!({})).await.data.unwrap();
    assert!(snap["messages"].as_array().unwrap().iter().any(|m| m["kind"] == "js_dialog"));

    let found = h.ok("find_element", json!({ "target": { "by": "text", "text": "Item A", "exact": true } })).await;
    let item_ref = found.target.unwrap().element.ref_id;
    h.ok("click", json!({ "target": { "by": "auto", "query": "Refresh list" } })).await;
    let r = h.run("click", json!({ "target": { "by": "ref", "ref": item_ref } })).await;
    assert_eq!(kind(&r), ErrorKind::StaleElement);

    let console = h.ok("get_console_errors", json!({})).await.data.unwrap();
    assert!(console["errors"].as_array().unwrap().iter().any(|e| e["text"] == "boom from page"), "{console}");
    let net = h.ok("get_network_failures", json!({})).await.data.unwrap();
    assert!(
        net["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["status"] == 404 && f["url"].as_str().unwrap().ends_with("/missing.json")),
        "{net}"
    );

    // The page set window.__sb; the helper lives in an isolated world and is unaffected.
    h.ok("find_element", json!({ "target": { "by": "css", "selector": "h1" } })).await;

    let shot = h.ok("get_screenshot", json!({ "name": "tricky", "full_page": true })).await;
    let bytes = std::fs::read(shot.data.unwrap()["path"].as_str().unwrap()).unwrap();
    assert_eq!(&bytes[..4], b"\x89PNG");

    let tree = h.ok("get_accessibility_tree", json!({ "max_nodes": 50 })).await.data.unwrap();
    assert!(tree["nodes"].as_array().unwrap().iter().any(|n| n["role"] == "button"));

    let report = h.ok("get_test_report", json!({})).await.data.unwrap();
    assert_eq!(report["verdict"], "failed", "the approval refusal counts as a failed action");
    assert_eq!(report["console_errors"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn navigation_errors_and_policy() {
    let Some(mut h) = harness().await else { return };
    let r = h.ok("navigate", json!({ "url": format!("{}/nope", h.base) })).await;
    assert_eq!(r.data.unwrap()["status"], 404);
    assert!(r.notes.iter().any(|n| n.contains("404")));

    let r = h.run("navigate", json!({ "url": "http://127.0.0.1:9/" })).await;
    assert_eq!(kind(&r), ErrorKind::NavigationFailed);

    let r = h.run("navigate", json!({ "url": "file:///etc/hostname" })).await;
    assert_eq!(kind(&r), ErrorKind::PermissionDenied);

    // Reported URL and title must be the new document's, never a stale or placeholder value.
    for i in 0..10 {
        let (path, title) = if i % 2 == 0 { ("/form", "Sign up") } else { ("/tricky", "Tricky") };
        let url = format!("{}{path}?{i}", h.base);
        let r = h.ok("navigate", json!({ "url": url })).await;
        let page = r.page.unwrap();
        assert_eq!((page.url.as_str(), page.title.as_str()), (url.as_str(), title));
        assert_eq!(r.data.unwrap()["title"], title);
    }

    let caps = h.ok("get_capabilities", json!({})).await.data.unwrap();
    assert!(caps["engine_version"].as_str().unwrap().contains("Chrome"));
    h.mgr.close_all().await;
}
