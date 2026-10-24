//! The warm-browser daemon: attach speed, isolation between clients, refusals,
//! authentication and cleanup. Runs on Linux, macOS and Windows.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use swiftbrowse_chromium::daemon;

const BIN: &str = env!("CARGO_BIN_EXE_swiftbrowse");

/// Serves /set (stores a cookie and localStorage) and /check (reports them in the title).
fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        // One thread per connection: Chromium opens speculative connections that
        // send nothing, which would block a sequential server.
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let body = if req.starts_with("GET /set") {
                    "<title>set</title><script>document.cookie='k=1; path=/'; localStorage.setItem('k','1');</script>"
                } else {
                    "<title>check</title><script>document.title = (document.cookie || 'no-cookie') + '|' + (localStorage.getItem('k') || 'no-storage');</script>"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    format!("http://{addr}")
}

fn cli(endpoint: &str, args: &[&str]) -> (Value, Duration) {
    let t = Instant::now();
    let out = Command::new(BIN).args(args).env("SWIFTBROWSE_DAEMON_SOCKET", endpoint).output().unwrap();
    (serde_json::from_slice(&out.stdout).unwrap_or(Value::Null), t.elapsed())
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

#[test]
fn warm_daemon_is_fast_isolated_and_cleans_up() {
    if swiftbrowse_chromium::launch::discover().is_none() {
        assert!(std::env::var("SWIFTBROWSE_SKIP_BROWSER_TESTS").is_ok_and(|v| v == "1"), "no Chromium found");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let endpoint = if cfg!(windows) {
        format!(r"\\.\pipe\swiftbrowse-test-{}", std::process::id())
    } else {
        dir.path().join("d.sock").display().to_string()
    };
    // The in-process client below reads the same variable the CLI gets per command.
    std::env::set_var("SWIFTBROWSE_DAEMON_SOCKET", &endpoint);
    let ep = endpoint.as_str();
    let base = serve();
    let allow = ["--allow-host", "127.0.0.1"];
    let snapshot = |path: &str| cli(ep, &[&["snapshot", &format!("{base}{path}")][..], &allow[..]].concat());

    // First run starts the daemon; later runs attach to it.
    let (first, cold) = snapshot("/set");
    assert_eq!(first["ok"], true, "{first}");
    let mut warm = Duration::MAX;
    for _ in 0..3 {
        let (r, t) = snapshot("/check");
        assert_eq!(r["ok"], true, "{r}");
        // A new client never sees an earlier client's cookies or storage.
        assert!(r["data"]["text"].as_str().unwrap().starts_with("page: no-cookie|no-storage"), "{r}");
        warm = warm.min(t);
    }
    assert!(warm < cold, "attach ({warm:?}) should beat a cold start ({cold:?})");
    // A genuinely blank page costs at most the empty-page give-up wait once.
    assert!(warm < Duration::from_millis(2500), "warm run took {warm:?}");

    let (status, _) = cli(ep, &["daemon", "status"]);
    assert_eq!(status["running"], true, "{status}");
    assert!(status["status"]["sessions_served"].as_u64().unwrap() >= 4, "{status}");

    // Browser-wide or foreign commands are refused.
    for method in ["Browser.close", "Target.getTargets", "Target.setDiscoverTargets"] {
        let r = block(daemon::raw(json!({ "id": 7, "method": method, "params": {} }))).unwrap();
        assert!(r["error"]["message"].as_str().unwrap().contains("not allowed"), "{method}: {r}");
    }
    let foreign =
        json!({ "id": 8, "method": "Runtime.evaluate", "sessionId": "not-mine", "params": { "expression": "1" } });
    let r = block(daemon::raw(foreign)).unwrap();
    assert!(r["error"].is_object(), "{r}");

    // Stop: the daemon goes away and new clients can't reach it.
    cli(ep, &["daemon", "stop"]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while block(daemon::call("SwiftBrowse.status")).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(block(daemon::call("SwiftBrowse.status")).is_none(), "daemon still running");
    if cfg!(unix) {
        assert!(!Path::new(ep).exists(), "socket removed");
    }
}

/// Without the token a client gets nothing, even from the right endpoint.
#[cfg(unix)]
#[test]
fn clients_without_the_token_are_dropped() {
    if swiftbrowse_chromium::launch::discover().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ep = dir.path().join("t.sock").display().to_string();
    let (r, _) = cli(&ep, &["snapshot", "data:text/html,hi"]);
    assert_eq!(r["ok"], true, "{r}");
    let mut s = std::os::unix::net::UnixStream::connect(&ep).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(b"{\"id\":1,\"method\":\"SwiftBrowse.hello\",\"params\":{\"token\":\"guess\"}}\0").unwrap();
    // The daemon may already have dropped us after the bad hello: then this write fails.
    let _ = s.write_all(b"{\"id\":2,\"method\":\"SwiftBrowse.status\"}\0");
    let mut buf = Vec::new();
    let n = s.read_to_end(&mut buf).unwrap_or(0);
    assert_eq!(n, 0, "daemon answered an unauthenticated client: {}", String::from_utf8_lossy(&buf));
    cli(&ep, &["daemon", "stop"]);
}
