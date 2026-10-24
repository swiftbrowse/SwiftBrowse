//! `swiftbrowse test` end to end: the example suite passes, failures leave
//! evidence and a non-zero exit, retries turn a first failure into "flaky",
//! and the JUnit and JSON reports agree.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_swiftbrowse");

fn browser_available() -> bool {
    if swiftbrowse_chromium::launch::discover().is_some() {
        return true;
    }
    assert!(std::env::var("SWIFTBROWSE_SKIP_BROWSER_TESTS").is_ok_and(|v| v == "1"), "no Chromium found");
    false
}

fn run(args: &[&str], cwd: &Path) -> (i32, String) {
    let out = Command::new(BIN).args(args).current_dir(cwd).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

fn results(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("results.json")).unwrap()).unwrap()
}

#[test]
fn example_suite_passes_in_parallel() {
    if !browser_available() {
        return;
    }
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/e2e");
    let out = tempfile::tempdir().unwrap();
    let out_dir = out.path().join("results");
    let (code, stdout) =
        run(&["test", "tests", "--serve", "site", "-j", "4", "--output-dir", out_dir.to_str().unwrap()], &examples);
    assert_eq!(code, 0, "{stdout}");
    let r = results(&out_dir);
    assert_eq!(r["summary"]["passed"], 12, "{stdout}");
    assert_eq!(r["summary"]["failed"], 0);
}

/// Answers `/page` with a title that changes after the first request.
fn flaky_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let hits = hits.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let body = if req.starts_with("GET /page") {
                    let n = hits.fetch_add(1, Ordering::SeqCst);
                    format!("<title>{}</title><h1>Hello</h1>", if n == 0 { "broken" } else { "fixed" })
                } else {
                    String::new()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    format!("http://{addr}/")
}

#[test]
fn failures_retries_and_reports() {
    if !browser_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let base = flaky_server();
    std::fs::write(
        dir.path().join("demo.test.json"),
        r#"{ "name": "demo", "tests": [
             { "name": "flaky title", "steps": [
                 { "op": "navigate", "url": "page" },
                 { "op": "expect", "condition": { "type": "title_equals", "value": "fixed" }, "timeout_ms": 200 } ] },
             { "name": "always fails", "steps": [
                 { "op": "navigate", "url": "page" },
                 { "op": "click", "target": "Missing button" } ] },
             { "name": "skipped", "skip": "not ready", "steps": [] }
           ] }"#,
    )
    .unwrap();
    let out_dir = dir.path().join("out");
    let junit = dir.path().join("junit.xml");
    let html = dir.path().join("report.html");
    let (code, stdout) = run(
        &[
            "test",
            "--base-url",
            &base,
            "--retries",
            "1",
            "--timeout-ms",
            "500",
            "-j",
            "1",
            "--output-dir",
            out_dir.to_str().unwrap(),
            "--junit",
            junit.to_str().unwrap(),
            "--html",
            html.to_str().unwrap(),
        ],
        dir.path(),
    );
    assert_eq!(code, 1, "{stdout}");
    let r = results(&out_dir);
    let s = &r["summary"];
    assert_eq!(
        (s["passed"].as_u64(), s["flaky"].as_u64(), s["failed"].as_u64(), s["skipped"].as_u64()),
        (Some(0), Some(1), Some(1), Some(1)),
        "{stdout}"
    );

    let failed = r["tests"].as_array().unwrap().iter().find(|t| t["name"] == "always fails").unwrap();
    assert_eq!(failed["attempts"].as_array().unwrap().len(), 2, "retried once");
    let last = &failed["attempts"][1];
    assert!(last["error"].as_str().unwrap().contains("no element matched"), "{last}");
    assert!(Path::new(last["screenshot"].as_str().unwrap()).exists(), "failure screenshot kept");
    assert!(last["page"].as_str().unwrap().contains("Hello"), "page snapshot kept");
    assert!(stdout.contains("1 failed, 1 flaky, 1 skipped"), "{stdout}");

    let xml = std::fs::read_to_string(&junit).unwrap();
    assert!(xml.contains(r#"tests="3" failures="1" skipped="1""#), "{xml}");
    let page = std::fs::read_to_string(&html).unwrap();
    assert!(page.contains("data:image/png;base64,"), "screenshot embedded in the HTML report");

    // A config error is exit code 2, before any browser starts.
    std::fs::write(
        dir.path().join("bad.test.json"),
        r#"{ "tests": [ { "name": "x", "steps": [ { "op": "teleport" } ] } ] }"#,
    )
    .unwrap();
    let (code, _) = run(&["test", "bad.test.json"], dir.path());
    assert_eq!(code, 2);
}
