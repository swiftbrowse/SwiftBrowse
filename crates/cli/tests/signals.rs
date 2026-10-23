//! A killed MCP server must not leave browsers or profiles behind.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn profiles_of(pid: u32) -> Vec<PathBuf> {
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| std::fs::read_to_string(p.join(".swiftbrowse-owner")).is_ok_and(|o| o.trim() == pid.to_string()))
        .collect()
}

/// Processes whose command line mentions the given profile directory.
fn browsers_using(profile: &Path) -> usize {
    let needle = profile.display().to_string();
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join("cmdline")).ok())
        .filter(|c| String::from_utf8_lossy(c).contains(&needle))
        .count()
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

fn run(signal: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_swiftbrowse"))
        .args(["mcp", "--evidence-dir", std::env::temp_dir().join("sb-signal-test").to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    for msg in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"navigate","arguments":{"url":"data:text/html,hi"}}}"#,
    ] {
        writeln!(stdin, "{msg}").unwrap();
        let line = out.next().unwrap().unwrap();
        assert!(line.contains("\"result\""), "{line}");
    }
    let pid = child.id();
    let profiles = profiles_of(pid);
    assert_eq!(profiles.len(), 1, "one prewarmed default session");
    let profile = profiles[0].clone();
    assert!(browsers_using(&profile) > 0);

    Command::new("kill").args([&format!("-{signal}"), &pid.to_string()]).status().unwrap();
    child.wait().unwrap();
    assert!(wait_until(|| browsers_using(&profile) == 0), "{signal}: browser outlived the server");
    if signal != "KILL" {
        assert!(wait_until(|| !profile.exists()), "{signal}: profile not removed");
    }
}

#[test]
fn signals_do_not_leak_browsers() {
    if swiftbrowse_chromium::launch::discover().is_none() {
        assert!(std::env::var("SWIFTBROWSE_SKIP_BROWSER_TESTS").is_ok_and(|v| v == "1"), "no Chromium found");
        return;
    }
    for signal in ["TERM", "INT", "HUP", "KILL"] {
        run(signal);
    }
}
