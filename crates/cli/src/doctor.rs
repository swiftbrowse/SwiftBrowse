//! `swiftbrowse doctor`: report what this machine can actually run.

use std::process::{Command, ExitCode};

use serde_json::{json, Value};
use swiftbrowse_chromium::launch;

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn check(name: &str, status: &str, detail: impl Into<String>) -> Value {
    json!({ "check": name, "status": status, "detail": detail.into() })
}

fn desktop_webview() -> Value {
    if cfg!(target_os = "linux") {
        match output("pkg-config", &["--modversion", "webkit2gtk-4.1"]) {
            Some(v) => check("desktop_webview", "ok", format!("WebKitGTK (webkit2gtk-4.1) {v}")),
            None => check(
                "desktop_webview",
                "missing",
                "webkit2gtk-4.1 not found; the desktop shell needs it (e.g. apt install libwebkit2gtk-4.1-dev). Cloud automation does not.",
            ),
        }
    } else if cfg!(target_os = "macos") {
        let v = output("sw_vers", &["-productVersion"]).unwrap_or_else(|| "unknown".into());
        check("desktop_webview", "ok", format!("WKWebView (system WebKit, macOS {v})"))
    } else if cfg!(target_os = "windows") {
        let key = r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
        match output("reg", &["query", key, "/v", "pv"]) {
            Some(out) => {
                let v = out.split_whitespace().last().unwrap_or("unknown").to_string();
                check("desktop_webview", "ok", format!("WebView2 Runtime {v}"))
            }
            None => check("desktop_webview", "missing", "WebView2 Runtime not found in the machine-wide registry key"),
        }
    } else {
        check("desktop_webview", "unknown", "unsupported platform")
    }
}

pub fn run(as_json: bool) -> ExitCode {
    let mut checks = vec![check("platform", "ok", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH))];

    let chromium = launch::discover();
    let mut ready = false;
    match &chromium {
        Some(path) => {
            let version = output(&path.display().to_string(), &["--version"]);
            ready = version.is_some();
            checks.push(match version {
                Some(v) => check("chromium", "ok", format!("{v} at {}", path.display())),
                None => check(
                    "chromium",
                    "error",
                    format!("{} exists but `--version` failed (missing shared libraries?)", path.display()),
                ),
            });
        }
        None => checks.push(check(
            "chromium",
            "missing",
            format!(
                "no Chromium found; install one (e.g. `npx playwright install chromium`) or set {}",
                launch::ENV_EXECUTABLE
            ),
        )),
    }
    match launch::discover_headless_shell() {
        Some(path) => {
            let v = output(&path.display().to_string(), &["--version"]).unwrap_or_else(|| "version unknown".into());
            checks.push(check("headless_shell", "ok", format!("{v} at {} (used for headless sessions)", path.display())));
        }
        None => checks.push(check(
            "headless_shell",
            "warn",
            "not found: headless sessions use full Chrome, which starts ~3x slower (`npx playwright install chromium-headless-shell`)",
        )),
    }
    match launch::no_sandbox_reason() {
        Some(reason) => checks.push(check(
            "sandbox",
            "warn",
            format!("{reason}: Chromium will start with --no-sandbox; rely on a container or VM boundary"),
        )),
        None => checks.push(check("sandbox", "ok", "Chromium sandbox will be used")),
    }
    let display = cfg!(any(target_os = "macos", target_os = "windows"))
        || std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    checks.push(if display {
        check("display", "ok", "headed mode available")
    } else {
        check("display", "warn", "no DISPLAY/WAYLAND_DISPLAY: headless only")
    });
    checks.push(desktop_webview());

    let summary = json!({
        "cloud_automation_ready": ready,
        "checks": checks,
    });
    if as_json {
        println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
    } else {
        for c in summary["checks"].as_array().into_iter().flatten() {
            let mark = match c["status"].as_str() {
                Some("ok") => "✓",
                Some("warn") => "!",
                _ => "✗",
            };
            println!("{mark} {:<16} {}", c["check"].as_str().unwrap_or(""), c["detail"].as_str().unwrap_or(""));
        }
        println!("\ncloud automation ready: {}", if ready { "yes" } else { "no" });
    }
    if ready {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
