//! The warm-browser daemon: attach speed, isolation between clients, refusals, cleanup.
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

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
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            });
        }
    });
    format!("http://{addr}")
}

fn cli(socket: &Path, args: &[&str]) -> (Value, Duration) {
    let t = Instant::now();
    let out = Command::new(BIN).args(args).env("SWIFTBROWSE_DAEMON_SOCKET", socket).output().unwrap();
    let elapsed = t.elapsed();
    (serde_json::from_slice(&out.stdout).unwrap_or(Value::Null), elapsed)
}

fn raw(socket: &Path, msg: Value) -> Value {
    let mut s = UnixStream::connect(socket).unwrap();
    let mut bytes = msg.to_string().into_bytes();
    bytes.push(0);
    s.write_all(&bytes).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = s.read(&mut chunk).unwrap();
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.iter().position(|b| *b == 0) {
            return serde_json::from_slice(&buf[..end]).unwrap();
        }
    }
}

#[test]
fn warm_daemon_is_fast_isolated_and_cleans_up() {
    if swiftbrowse_chromium::launch::discover().is_none() {
        assert!(std::env::var("SWIFTBROWSE_SKIP_BROWSER_TESTS").is_ok_and(|v| v == "1"), "no Chromium found");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let socket: PathBuf = dir.path().join("d.sock");
    let base = serve();
    let allow = ["--allow-host", "127.0.0.1"];

    // First run starts the daemon; later runs attach to it.
    let (first, cold) = cli(&socket, &[&["snapshot", &format!("{base}/set")][..], &allow[..]].concat());
    assert_eq!(first["ok"], true, "{first}");
    let mut warm = Duration::MAX;
    for _ in 0..3 {
        let (r, t) = cli(&socket, &[&["snapshot", &format!("{base}/check")][..], &allow[..]].concat());
        assert_eq!(r["ok"], true, "{r}");
        // A new client never sees an earlier client's cookies or storage.
        assert!(r["data"]["text"].as_str().unwrap().starts_with("page: no-cookie|no-storage"), "{r}");
        warm = warm.min(t);
    }
    assert!(warm < cold, "attach ({warm:?}) should beat a cold start ({cold:?})");
    // A genuinely blank page costs at most the empty-page give-up wait once.
    assert!(warm < Duration::from_millis(2500), "warm run took {warm:?}");

    let (status, _) = cli(&socket, &["daemon", "status"]);
    assert_eq!(status["running"], true, "{status}");
    assert!(status["status"]["sessions_served"].as_u64().unwrap() >= 4, "{status}");

    // Browser-wide or foreign commands are refused.
    for method in ["Browser.close", "Target.getTargets", "Target.setDiscoverTargets"] {
        let r = raw(&socket, json!({ "id": 7, "method": method, "params": {} }));
        assert!(r["error"]["message"].as_str().unwrap().contains("not allowed"), "{method}: {r}");
    }
    let r = raw(
        &socket,
        json!({ "id": 8, "method": "Runtime.evaluate", "sessionId": "not-mine", "params": { "expression": "1" } }),
    );
    assert!(r["error"].is_object(), "{r}");

    // Stop: the daemon and its browser go away.
    let pid = status["status"]["pid"].as_u64().unwrap();
    cli(&socket, &["daemon", "stop"]);
    let gone = |p: u64| !Path::new(&format!("/proc/{p}")).exists();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gone(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(gone(pid), "daemon still running");
    assert!(!socket.exists(), "socket removed");
}
