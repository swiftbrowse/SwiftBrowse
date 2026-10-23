//! Finding and starting a Chromium binary.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use swiftbrowse_core::{CoreError, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub const ENV_EXECUTABLE: &str = "SWIFTBROWSE_CHROMIUM";
pub const ENV_NO_SANDBOX: &str = "SWIFTBROWSE_NO_SANDBOX";

/// Locates a Chromium-family browser. `SWIFTBROWSE_CHROMIUM` wins; then
/// Playwright-managed builds (pinned per Playwright release); then common
/// install locations and `PATH`.
pub fn discover() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(ENV_EXECUTABLE) {
        return Some(PathBuf::from(p));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH") {
        roots.push(PathBuf::from(p));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        roots.push(home.join(".cache/ms-playwright"));
        roots.push(home.join("Library/Caches/ms-playwright"));
        roots.push(home.join("AppData/Local/ms-playwright"));
    }
    for root in roots {
        if let Some(p) = newest_playwright_chromium(&root) {
            return Some(p);
        }
    }
    let fixed: &[&str] = &[
        "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    if let Some(p) = fixed.iter().map(PathBuf::from).find(|p| p.is_file()) {
        return Some(p);
    }
    let names = ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable", "chrome"];
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).flat_map(|dir| names.iter().map(move |n| dir.join(n))).find(|p| p.is_file())
}

fn newest_playwright_chromium(root: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<(u32, PathBuf)> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let rev = name.strip_prefix("chromium-")?.parse().ok()?;
            Some((rev, e.path()))
        })
        .collect();
    dirs.sort();
    dirs.into_iter().rev().find_map(|(_, dir)| {
        [
            "chrome-linux64/chrome",
            "chrome-linux/chrome",
            "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
            "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            "chrome-win/chrome.exe",
            "chrome-win64/chrome.exe",
        ]
        .iter()
        .map(|rel| dir.join(rel))
        .find(|p| p.is_file())
    })
}

/// Chromium's sandbox cannot start as root; running as root therefore needs
/// `--no-sandbox`, which we only add automatically there and report as a warning.
pub fn needs_no_sandbox() -> bool {
    if std::env::var(ENV_NO_SANDBOX).is_ok_and(|v| v == "1") {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            return status
                .lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .is_some_and(|euid| euid == "0");
        }
    }
    false
}

pub struct Launched {
    pub child: Child,
    pub ws_url: String,
    pub profile: tempfile::TempDir,
}

pub async fn launch(exe: &Path, headless: bool, no_sandbox: bool, extra: &[String]) -> Result<Launched> {
    let profile = tempfile::Builder::new()
        .prefix("swiftbrowse-profile-")
        .tempdir()
        .map_err(|e| CoreError::backend(format!("cannot create profile directory: {e}")))?;
    let mut cmd = Command::new(exe);
    cmd.arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--no-startup-window",
            "--no-service-autorun",
            "--disable-field-trial-config",
            "--disable-background-networking",
            // Back/forward cache freezes the old page on every navigation; automation
            // wants a fresh document and pays for the freeze otherwise.
            "--disable-back-forward-cache",
            "--disable-background-timer-throttling",
            "--disable-backgrounding-occluded-windows",
            "--disable-renderer-backgrounding",
            "--disable-breakpad",
            "--disable-client-side-phishing-detection",
            "--disable-component-extensions-with-background-pages",
            "--disable-component-update",
            "--disable-default-apps",
            "--disable-dev-shm-usage",
            "--disable-extensions",
            "--disable-hang-monitor",
            "--disable-ipc-flooding-protection",
            "--disable-prompt-on-repost",
            "--disable-sync",
            "--disable-features=Translate,MediaRouter,DialMediaRouteProvider,GlobalMediaControls,OptimizationHints,PaintHolding,LensOverlay,NetworkTimeServiceQuerying",
            "--force-color-profile=srgb",
            "--metrics-recording-only",
            "--mute-audio",
            "--password-store=basic",
            "--use-mock-keychain",
        ]);
    if headless {
        cmd.args(["--headless=new", "--hide-scrollbars"]);
    }
    if no_sandbox {
        cmd.arg("--no-sandbox");
    }
    cmd.args(extra);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| CoreError::backend(format!("cannot start {}: {e}", exe.display())))?;
    let stderr = child.stderr.take().expect("piped stderr");
    let mut lines = BufReader::new(stderr).lines();
    let mut tail: Vec<String> = Vec::new();

    let found = tokio::time::timeout(Duration::from_secs(30), async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(idx) = line.find("ws://") {
                return Some(line[idx..].trim().to_string());
            }
            tail.push(line);
            if tail.len() > 20 {
                tail.remove(0);
            }
        }
        None
    })
    .await;

    let ws_url = match found {
        Ok(Some(url)) => url,
        Ok(None) | Err(_) => {
            let _ = child.kill().await;
            return Err(CoreError::backend(format!(
                "browser exited or did not report a DevTools endpoint; last output:\n{}",
                tail.join("\n")
            ))
            .with_details(serde_json::json!({ "executable": exe.display().to_string(), "no_sandbox": no_sandbox })));
        }
    };
    // Keep draining stderr so the browser never blocks on a full pipe.
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

    Ok(Launched { child, ws_url, profile })
}
