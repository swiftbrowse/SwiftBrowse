use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use swiftbrowse_core::mock::{Effect, MockElement, MockFactory, MockPage};
use swiftbrowse_core::result::VerificationStatus;
use swiftbrowse_core::{ActionResult, ErrorKind, Policy, Request, SessionManager, SessionOptions};

fn pages() -> HashMap<String, MockPage> {
    let mut m = HashMap::new();
    m.insert(
        "https://app.test/".into(),
        MockPage {
            title: "Home".into(),
            elements: vec![
                MockElement::button("e1", "Delete").on_click(Effect::Remove("e5".into())),
                MockElement::button("e2", "Delete"),
                MockElement::button("e3", "Menu").hidden(),
                MockElement::button("e4", "Menu"),
                MockElement::new("e5", "p", None, "Row one"),
                MockElement::textbox("e6", "Email"),
                MockElement::button("e7", "Continue").test_id("next").on_click(Effect::Navigate { url: "https://app.test/done".into() }),
                MockElement::button("e8", "Place order"),
                MockElement::button("e9", "Late").appears_after(Duration::from_millis(150)),
                MockElement::button("e10", "Disabled").disabled(),
                MockElement::button("e11", "Reveal").on_click(Effect::Show("e12".into())),
                MockElement::new("e12", "div", Some("alert"), "Saved!").hidden(),
            ],
        },
    );
    m.insert("https://app.test/done".into(), MockPage { title: "Done".into(), elements: vec![] });
    m
}

async fn setup() -> (SessionManager, Arc<MockFactory>, String) {
    let factory = Arc::new(MockFactory::new(pages()));
    let policy = Policy {
        evidence_dir: tempfile::tempdir().unwrap().keep(),
        default_timeout_ms: 400,
        ..Policy::default()
    };
    let mut mgr = SessionManager::new(factory.clone(), policy);
    let id = mgr.create(None, &SessionOptions::default()).await.unwrap().session_id;
    let r = run(&mut mgr, &id, "navigate", json!({ "url": "https://app.test/" })).await;
    assert!(r.ok, "{r:?}");
    (mgr, factory, id)
}

async fn run(mgr: &mut SessionManager, id: &str, op: &str, args: Value) -> ActionResult {
    mgr.execute(id, Request::from_op(op, args).unwrap()).await
}

fn kind(r: &ActionResult) -> ErrorKind {
    r.error.as_ref().expect("expected an error").kind
}

#[tokio::test]
async fn ambiguous_match_is_reported_not_guessed() {
    let (mut mgr, f, id) = setup().await;
    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "role", "role": "button", "name": "Delete", "exact": true }, "approved": true })).await;
    assert_eq!(kind(&r), ErrorKind::Ambiguous);
    assert_eq!(r.error.unwrap().details["total"], 2);
    assert!(f.last.lock().unwrap().as_ref().unwrap().state.lock().unwrap().clicks.is_empty());

    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "role", "role": "button", "name": "Delete", "exact": true, "nth": 1 }, "approved": true })).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.target.unwrap().element.ref_id, "e2");
}

#[tokio::test]
async fn hidden_duplicates_are_ignored_when_one_is_visible() {
    let (mut mgr, _f, id) = setup().await;
    let r = run(&mut mgr, &id, "find_element", json!({ "target": { "by": "auto", "query": "Menu" } })).await;
    assert!(r.ok, "{r:?}");
    let t = r.target.unwrap();
    assert_eq!(t.element.ref_id, "e4");
    assert_eq!(t.candidates, 2);
    assert_eq!(t.strategy, "auto:accessible_name");
}

#[tokio::test]
async fn auto_follows_locator_priority() {
    let (mut mgr, _f, id) = setup().await;
    let r = run(&mut mgr, &id, "find_element", json!({ "target": { "by": "auto", "query": "Email" } })).await;
    assert_eq!(r.target.unwrap().strategy, "auto:accessible_name");
    let r = run(&mut mgr, &id, "find_element", json!({ "target": { "by": "auto", "query": "next" } })).await;
    assert_eq!(r.target.unwrap().strategy, "auto:test_id");
    let r = run(&mut mgr, &id, "find_element", json!({ "target": { "by": "auto", "query": "row" } })).await;
    assert_eq!(r.target.unwrap().strategy, "auto:partial_text");
}

#[tokio::test]
async fn consequential_click_requires_approval() {
    let (mut mgr, f, id) = setup().await;
    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "auto", "query": "Place order" } })).await;
    assert_eq!(kind(&r), ErrorKind::ApprovalRequired);
    assert!(!r.error.unwrap().recoverable);
    assert!(f.last.lock().unwrap().as_ref().unwrap().state.lock().unwrap().clicks.is_empty(), "must not click");

    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "auto", "query": "Place order" }, "approved": true })).await;
    assert!(r.ok);
}

#[tokio::test]
async fn click_without_expect_is_not_verified_and_with_expect_is() {
    let (mut mgr, _f, id) = setup().await;
    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "test_id", "id": "next" } })).await;
    assert!(r.ok);
    assert_eq!(r.verification.unwrap().status, VerificationStatus::NotChecked);

    run(&mut mgr, &id, "navigate", json!({ "url": "https://app.test/" })).await;
    let r = run(&mut mgr, &id, "click", json!({
        "target": { "by": "test_id", "id": "next" },
        "expect": { "type": "url_contains", "value": "/done" }
    }))
    .await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.verification.unwrap().status, VerificationStatus::Passed);
    assert_eq!(r.page.unwrap().title, "Done");
}

#[tokio::test]
async fn failed_expectation_is_a_verification_failure_not_a_retry() {
    let (mut mgr, f, id) = setup().await;
    let r = run(&mut mgr, &id, "click", json!({
        "target": { "by": "auto", "query": "Menu" },
        "expect": { "type": "url_contains", "value": "/never" },
        "timeout_ms": 100
    }))
    .await;
    assert_eq!(kind(&r), ErrorKind::VerificationFailed);
    assert_eq!(f.last.lock().unwrap().as_ref().unwrap().state.lock().unwrap().clicks.len(), 1, "clicked exactly once");
}

#[tokio::test]
async fn waits_for_late_elements_and_times_out_on_disabled_ones() {
    let (mut mgr, _f, id) = setup().await;
    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "auto", "query": "Late" } })).await;
    assert!(r.ok, "{r:?}");
    assert!(r.notes.iter().any(|n| n.contains("waited")));

    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "auto", "query": "Disabled" }, "timeout_ms": 120 })).await;
    assert_eq!(kind(&r), ErrorKind::Timeout);
    assert!(r.error.unwrap().message.contains("disabled"));
}

#[tokio::test]
async fn stale_refs_are_reported() {
    let (mut mgr, _f, id) = setup().await;
    let r = run(&mut mgr, &id, "click", json!({ "target": { "by": "role", "role": "button", "name": "Delete", "nth": 0 }, "approved": true })).await;
    assert!(r.ok);
    let r = run(&mut mgr, &id, "find_element", json!({ "target": { "by": "ref", "ref": "e5" } })).await;
    assert_eq!(kind(&r), ErrorKind::StaleElement);
}

#[tokio::test]
async fn fill_is_verified_and_assertions_feed_the_report() {
    let (mut mgr, _f, id) = setup().await;
    let r = run(&mut mgr, &id, "fill", json!({ "target": { "by": "label", "text": "Email" }, "value": "a@b.c" })).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.verification.unwrap().status, VerificationStatus::Passed);

    run(&mut mgr, &id, "click", json!({ "target": { "by": "auto", "query": "Reveal" } })).await;
    let r = run(&mut mgr, &id, "assert_page_state", json!({
        "name": "saved banner",
        "conditions": [
            { "type": "visible", "target": { "by": "role", "role": "alert" } },
            { "type": "value_equals", "target": { "by": "label", "text": "Email" }, "value": "a@b.c" },
            { "type": "title_contains", "value": "Home" }
        ]
    }))
    .await;
    assert!(r.ok, "{r:?}");

    let r = run(&mut mgr, &id, "assert_page_state", json!({ "conditions": [{ "type": "text_visible", "text": "nope" }] })).await;
    assert_eq!(kind(&r), ErrorKind::VerificationFailed);

    let report = run(&mut mgr, &id, "get_test_report", json!({})).await.data.unwrap();
    assert_eq!(report["verdict"], "failed");
    assert_eq!(report["assertions"]["passed"], 1);
    assert_eq!(report["assertions"]["failed"], 1);
}

#[tokio::test]
async fn policy_and_validation_errors() {
    let (mut mgr, _f, id) = setup().await;
    let r = run(&mut mgr, &id, "navigate", json!({ "url": "file:///etc/passwd" })).await;
    assert_eq!(kind(&r), ErrorKind::PermissionDenied);
    let r = run(&mut mgr, &id, "find_element", json!({ "target": { "by": "css", "selector": "#x" } })).await;
    assert_eq!(kind(&r), ErrorKind::Unsupported);
    let r = run(&mut mgr, &id, "get_screenshot", json!({ "name": "../escape" })).await;
    assert_eq!(kind(&r), ErrorKind::InvalidArgument);
    let r = mgr.execute("missing", Request::from_op("get_capabilities", json!({})).unwrap()).await;
    assert_eq!(kind(&r), ErrorKind::SessionNotFound);
}

#[tokio::test]
async fn snapshot_diff_reports_changes() {
    let (mut mgr, _f, id) = setup().await;
    run(&mut mgr, &id, "get_page_snapshot", json!({})).await;
    run(&mut mgr, &id, "click", json!({ "target": { "by": "auto", "query": "Reveal" } })).await;
    let r = run(&mut mgr, &id, "get_page_snapshot", json!({ "since_last": true })).await;
    let added = &r.data.unwrap()["diff"]["added"];
    assert_eq!(added.as_array().unwrap().len(), 1);
    assert_eq!(added[0]["ref"], "e12");
}

#[tokio::test]
async fn session_limit_and_cleanup() {
    let factory = Arc::new(MockFactory::new(pages()));
    let mut mgr = SessionManager::new(factory, Policy::default());
    mgr.max_sessions = 2;
    mgr.create(None, &SessionOptions::default()).await.unwrap();
    mgr.create(Some("named".into()), &SessionOptions::default()).await.unwrap();
    let err = mgr.create(None, &SessionOptions::default()).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::SessionLimit);
    assert!(mgr.create(Some("../bad".into()), &SessionOptions::default()).await.is_err());
    mgr.close_all().await;
    assert!(mgr.list().is_empty());
}
