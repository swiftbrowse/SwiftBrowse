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
    for root in playwright_roots() {
        if let Some(p) = newest_playwright_build(&root, "chromium-", FULL_BUILDS) {
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

pub const ENV_HEADLESS_SHELL: &str = "SWIFTBROWSE_HEADLESS_SHELL";

const FULL_BUILDS: &[&str] = &[
    "chrome-linux64/chrome",
    "chrome-linux/chrome",
    "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    "chrome-win/chrome.exe",
    "chrome-win64/chrome.exe",
];

const SHELL_BUILDS: &[&str] = &[
    "chrome-headless-shell-linux64/chrome-headless-shell",
    "chrome-linux/headless_shell",
    "chrome-headless-shell-mac-arm64/chrome-headless-shell",
    "chrome-headless-shell-mac-x64/chrome-headless-shell",
    "chrome-headless-shell-win64/chrome-headless-shell.exe",
    "chrome-win/headless_shell.exe",
];

/// Finds Chromium's headless shell: the headless-only build Playwright also
/// uses by default. It starts about 3x faster than full Chrome in headless
/// mode. `SWIFTBROWSE_HEADLESS_SHELL` wins; `SWIFTBROWSE_CHROMIUM` disables it.
pub fn discover_headless_shell() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(ENV_HEADLESS_SHELL) {
        return Some(PathBuf::from(p));
    }
    if std::env::var_os(ENV_EXECUTABLE).is_some() {
        return None;
    }
    playwright_roots()
        .iter()
        .find_map(|root| newest_playwright_build(root, "chromium_headless_shell-", SHELL_BUILDS))
        .or_else(|| {
            let path = std::env::var_os("PATH")?;
            std::env::split_paths(&path).map(|dir| dir.join("chrome-headless-shell")).find(|p| p.is_file())
        })
}

/// Whether a binary is the headless shell (always headless; takes no `--headless`).
pub fn is_headless_shell(exe: &Path) -> bool {
    exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains("headless"))
}

fn playwright_roots() -> Vec<PathBuf> {
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
    roots
}

fn newest_playwright_build(root: &Path, prefix: &str, rels: &[&str]) -> Option<PathBuf> {
    let mut dirs: Vec<(u32, PathBuf)> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let rev = name.strip_prefix(prefix)?.parse().ok()?;
            Some((rev, e.path()))
        })
        .collect();
    dirs.sort();
    dirs.into_iter().rev().find_map(|(_, dir)| rels.iter().map(|rel| dir.join(rel)).find(|p| p.is_file()))
}

/// Chromium's sandbox cannot start as root; running as root therefore needs
/// `--no-sandbox`, which we only add automatically there and report as a warning.
/// Set once a browser in this process failed for lack of a usable sandbox.
pub static SANDBOX_UNUSABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Why Chromium must run without its sandbox here, if it must.
pub fn no_sandbox_reason() -> Option<&'static str> {
    if std::env::var(ENV_NO_SANDBOX).is_ok_and(|v| v == "1") {
        return Some("SWIFTBROWSE_NO_SANDBOX=1");
    }
    if SANDBOX_UNUSABLE.load(std::sync::atomic::Ordering::Relaxed) {
        return Some("the browser reported no usable sandbox");
    }
    #[cfg(target_os = "linux")]
    {
        let root = std::fs::read_to_string("/proc/self/status").is_ok_and(|status| {
            status.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|l| l.split_whitespace().nth(1)) == Some("0")
        });
        if root {
            return Some("running as root");
        }
        // Ubuntu 23.10+ (and GitHub's runners) deny unprivileged user namespaces
        // through AppArmor, except to browsers with their own profile; the
        // Playwright-managed builds have none.
        let restricted = std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
            .is_ok_and(|v| v.trim() == "1")
            || std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone").is_ok_and(|v| v.trim() == "0");
        if restricted {
            return Some("unprivileged user namespaces are restricted (AppArmor)");
        }
    }
    None
}

pub fn needs_no_sandbox() -> bool {
    no_sandbox_reason().is_some()
}

const PROFILE_PREFIX: &str = "swiftbrowse-profile-";
const OWNER_FILE: &str = ".swiftbrowse-owner";

/// Removes profiles left behind by SwiftBrowse processes that were killed
/// without cleaning up (e.g. SIGKILL). A profile is removed only when it names
/// an owner process that no longer exists.
fn sweep_orphaned_profiles() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(PROFILE_PREFIX) {
            continue;
        }
        let Ok(owner) = std::fs::read_to_string(entry.path().join(OWNER_FILE)) else { continue };
        let Ok(pid) = owner.trim().parse::<u32>() else { continue };
        if pid != std::process::id() && !process_alive(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // Signal 0 checks existence; EPERM means it exists but belongs to someone else.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 || *libc_errno() == libc::EPERM }
}

#[cfg(target_os = "linux")]
unsafe fn libc_errno() -> *mut i32 {
    libc::__errno_location()
}

#[cfg(target_os = "macos")]
unsafe fn libc_errno() -> *mut i32 {
    libc::__error()
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    // Without a cheap portable check, never delete: leaving a stale profile is safe.
    true
}

pub enum Transport {
    WebSocket(String),
    #[cfg(unix)]
    Pipe(tokio::net::unix::pipe::Sender, tokio::net::unix::pipe::Receiver),
}

pub struct Launched {
    pub child: Child,
    pub transport: Transport,
    pub profile: tempfile::TempDir,
    /// Last lines the browser wrote to stderr, for startup error messages.
    pub stderr_tail: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

/// `--remote-debugging-pipe` on Linux and macOS unless `SWIFTBROWSE_TRANSPORT=websocket`.
/// Windows uses a WebSocket on 127.0.0.1: handing the browser pipe handles
/// needs CRT handle inheritance that std's process API does not expose.
fn use_pipe() -> bool {
    cfg!(unix) && std::env::var("SWIFTBROWSE_TRANSPORT").map_or(true, |v| v != "websocket")
}

#[cfg(unix)]
fn pipe_pair() -> std::io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    #[cfg(target_os = "linux")]
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let rc = unsafe {
        // No pipe2 on macOS: set close-on-exec right after creating the pipe.
        let rc = libc::pipe(fds.as_mut_ptr());
        if rc == 0 {
            libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
        }
        rc
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (std::os::fd::OwnedFd::from_raw_fd(fds[0]), std::os::fd::OwnedFd::from_raw_fd(fds[1])) })
}

/// Windows: put the browser in a job that is killed when this process's job
/// handle closes, so a crashed or killed SwiftBrowse never leaves it running.
#[cfg(windows)]
fn kill_with_parent(child: &Child) {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    let Some(process) = child.raw_handle() else { return };
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        AssignProcessToJobObject(job, process as _);
        // The job handle is deliberately kept open for the life of this process.
    }
}

pub async fn launch(exe: &Path, headless: bool, no_sandbox: bool, extra: &[String]) -> Result<Launched> {
    sweep_orphaned_profiles();
    let profile = tempfile::Builder::new()
        .prefix(PROFILE_PREFIX)
        .tempdir()
        .map_err(|e| CoreError::backend(format!("cannot create profile directory: {e}")))?;
    let _ = std::fs::write(profile.path().join(OWNER_FILE), std::process::id().to_string());
    let pipe = use_pipe();
    // Pipe transport: commands go out the moment the process exists, with no
    // wait for a DevTools server to announce itself and no WebSocket handshake.
    #[cfg(unix)]
    let pipes = if pipe {
        let (cmd_r, cmd_w) = pipe_pair().map_err(|e| CoreError::backend(format!("cannot create pipe: {e}")))?;
        let (resp_r, resp_w) = pipe_pair().map_err(|e| CoreError::backend(format!("cannot create pipe: {e}")))?;
        Some((cmd_r, cmd_w, resp_r, resp_w))
    } else {
        None
    };
    let mut cmd = Command::new(exe);
    cmd.arg(if pipe { "--remote-debugging-pipe" } else { "--remote-debugging-port=0" })
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
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
        if !is_headless_shell(exe) {
            cmd.arg("--headless=new");
        }
        cmd.arg("--hide-scrollbars");
    }
    if no_sandbox {
        // The zygote exists to set up the sandbox; without one it only costs startup time.
        cmd.args(["--no-sandbox", "--no-zygote"]);
    }
    cmd.args(extra);
    if is_headless_shell(exe) && !pipe {
        // Over WebSocket the shell opens this tab at launch, so its renderer boots
        // while we wait to connect. Over a pipe we create the tab ourselves the
        // moment the browser can take commands; an extra startup tab would only
        // boot a second renderer.
        cmd.arg("about:blank");
    } else {
        cmd.arg("--no-startup-window");
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    // If SwiftBrowse dies without cleaning up (SIGKILL, crash), the kernel kills
    // the browser too instead of leaving it orphaned. The signal follows the
    // spawning thread, which is a long-lived runtime worker here.
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // The browser reads commands from fd 3 and writes replies to fd 4.
        let child_fds = pipes.as_ref().map(|(cmd_r, _, _, resp_w)| (cmd_r.as_raw_fd(), resp_w.as_raw_fd()));
        unsafe {
            cmd.pre_exec(move || {
                // On macOS the browser exits when its debugging pipe closes, which
                // covers the same case without a parent-death signal.
                #[cfg(target_os = "linux")]
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some((cmd_r, resp_w)) = child_fds {
                    // Move both above 3/4 first so neither dup2 clobbers the other.
                    let a = libc::fcntl(cmd_r, libc::F_DUPFD_CLOEXEC, 10);
                    let b = libc::fcntl(resp_w, libc::F_DUPFD_CLOEXEC, 10);
                    if a < 0 || b < 0 || libc::dup2(a, 3) < 0 || libc::dup2(b, 4) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }

    let mut child = cmd.spawn().map_err(|e| CoreError::backend(format!("cannot start {}: {e}", exe.display())))?;
    #[cfg(windows)]
    kill_with_parent(&child);
    let stderr = child.stderr.take().expect("piped stderr");
    let mut lines = BufReader::new(stderr).lines();
    let stderr_tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    #[cfg(unix)]
    if let Some((cmd_r, cmd_w, resp_r, resp_w)) = pipes {
        // The child has its own copies now.
        drop((cmd_r, resp_w));
        let to_browser = tokio::net::unix::pipe::Sender::from_owned_fd(cmd_w)
            .map_err(|e| CoreError::backend(format!("pipe setup failed: {e}")))?;
        let from_browser = tokio::net::unix::pipe::Receiver::from_owned_fd(resp_r)
            .map_err(|e| CoreError::backend(format!("pipe setup failed: {e}")))?;
        let tail = stderr_tail.clone();
        // Keep draining stderr so the browser never blocks on a full pipe.
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                let mut t = tail.lock().unwrap();
                t.push(line);
                if t.len() > 20 {
                    t.remove(0);
                }
            }
        });
        return Ok(Launched { child, transport: Transport::Pipe(to_browser, from_browser), profile, stderr_tail });
    }

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

    Ok(Launched { child, transport: Transport::WebSocket(ws_url), profile, stderr_tail })
}
