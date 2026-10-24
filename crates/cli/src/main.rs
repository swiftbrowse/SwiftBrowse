mod doctor;
mod sdk;
mod testrun;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use swiftbrowse_chromium::{ChromiumFactory, DaemonMode};
use swiftbrowse_core::{Policy, Request, SessionManager, SessionOptions};

#[derive(Parser)]
#[command(name = "swiftbrowse", version, about = "The AI browser for browser automation and end-to-end testing")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report the environment: OS, browser binaries, versions, desktop WebView dependencies.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Start a session and print the backend's capability report.
    Capabilities(#[command(flatten)] Common),
    /// Run a JSON script of operations and print every result plus the test report.
    Run {
        /// Script file, or `-` for stdin.
        script: PathBuf,
        #[command(flatten)]
        common: Common,
    },
    /// Run end-to-end tests: every `*.test.json` / `*.spec.json` under the given
    /// paths, in parallel sessions, with retries and list/JSON/JUnit/HTML reports.
    Test(Box<testrun::TestArgs>),
    /// Navigate and print a compact page snapshot.
    Snapshot {
        url: String,
        #[arg(long)]
        full: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Navigate and save a screenshot into the evidence directory.
    Screenshot {
        url: String,
        /// File name stem (no path separators).
        #[arg(long, default_value = "screenshot")]
        name: String,
        #[arg(long)]
        full_page: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Serve the Model Context Protocol over stdio.
    Mcp {
        /// Do not launch the default session's browser until the first tool call.
        #[arg(long)]
        no_prewarm: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Serve the SDK protocol (JSON lines on stdin/stdout) for the TypeScript and Python SDKs.
    Sdk {
        /// Test defaults, as in `swiftbrowse test`: no approval for consequential
        /// clicks, `evaluate` allowed, uploads from the working directory.
        #[arg(long)]
        test_defaults: bool,
        /// Print every operation with its JSON schema and exit (for code generation).
        #[arg(long)]
        schema: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Manage the warm-browser daemon that headless sessions attach to.
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Run the daemon in the foreground (clients start it automatically).
    Run {
        /// Exit after this many seconds without clients.
        #[arg(long, default_value_t = 600)]
        idle_timeout_secs: u64,
        #[arg(long)]
        chromium: Option<PathBuf>,
    },
    /// Show whether a daemon is running and what it is doing.
    Status,
    /// Stop the running daemon; its browser and all contexts close.
    Stop,
}

#[derive(Args, Clone)]
struct Common {
    /// Show the browser window (needs a display).
    #[arg(long)]
    headed: bool,
    /// Chromium binary; defaults to $SWIFTBROWSE_CHROMIUM, then discovery (see `doctor`).
    #[arg(long)]
    chromium: Option<PathBuf>,
    /// Directory for screenshots and other evidence.
    #[arg(long)]
    evidence_dir: Option<PathBuf>,
    /// Restrict navigation to these hosts (repeatable; `*.example.com` allowed).
    #[arg(long = "allow-host")]
    allow_hosts: Vec<String>,
    #[arg(long = "block-host")]
    block_hosts: Vec<String>,
    /// Permit file:// navigation.
    #[arg(long)]
    allow_file_urls: bool,
    /// Default per-operation timeout.
    #[arg(long)]
    timeout_ms: Option<u64>,
    /// Do not require approval for consequential clicks (CI against test environments only).
    #[arg(long)]
    no_approval: bool,
    /// Launch a browser for this process instead of attaching to the warm-browser
    /// daemon (also `SWIFTBROWSE_DAEMON=off`).
    #[arg(long)]
    no_daemon: bool,
    /// Allow the `evaluate` operation (page scripts). `run` and `test` always allow it.
    #[arg(long)]
    allow_evaluate: bool,
    /// Let `upload_file` read files from this directory (repeatable). Uploads are off without it.
    #[arg(long = "allow-upload-dir", value_name = "DIR")]
    upload_dirs: Vec<PathBuf>,
}

impl Common {
    fn manager(&self) -> SessionManager {
        self.manager_with(self.policy())
    }

    fn manager_with(&self, policy: Policy) -> SessionManager {
        let factory =
            ChromiumFactory { executable: self.chromium.clone(), daemon: self.daemon_mode(), ..Default::default() };
        SessionManager::new(Arc::new(factory), policy)
    }

    fn policy(&self) -> Policy {
        let mut policy = Policy::default();
        if let Some(dir) = &self.evidence_dir {
            policy.evidence_dir = dir.clone();
        }
        policy.allowed_hosts = self.allow_hosts.clone();
        policy.blocked_hosts = self.block_hosts.clone();
        if self.allow_file_urls {
            policy.allowed_schemes.push("file".into());
        }
        if let Some(t) = self.timeout_ms {
            policy.default_timeout_ms = t;
        }
        policy.require_approval_for_consequential = !self.no_approval;
        policy.allow_evaluate = self.allow_evaluate;
        policy.upload_roots = self.upload_dirs.clone();
        policy
    }

    /// Headless sessions attach to the warm-browser daemon unless told not to.
    fn daemon_mode(&self) -> DaemonMode {
        let off = self.no_daemon || self.headed || std::env::var("SWIFTBROWSE_DAEMON").is_ok_and(|v| v == "off");
        match std::env::current_exe() {
            Ok(program) if !off => {
                let mut args = vec!["daemon".to_string(), "run".to_string()];
                if let Some(c) = &self.chromium {
                    args.extend(["--chromium".to_string(), c.display().to_string()]);
                }
                DaemonMode::Auto { program, args }
            }
            _ => DaemonMode::Off,
        }
    }

    fn options(&self) -> SessionOptions {
        SessionOptions { headless: !self.headed, ..Default::default() }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    #[serde(default)]
    name: Option<String>,
    steps: Vec<Value>,
    #[serde(default = "yes")]
    stop_on_failure: bool,
}

fn yes() -> bool {
    true
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    print(&json!({ "ok": false, "error": msg.to_string() }));
    ExitCode::from(2)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor { json } => doctor::run(json),
        Command::Daemon { action } => daemon_command(action).await,
        Command::Mcp { no_prewarm, common } => {
            let mut server = swiftbrowse_mcp::McpServer::new(common.manager(), common.options());
            if !no_prewarm {
                server.prewarm();
            }
            let code = match swiftbrowse_mcp::serve_stdio(server).await {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("swiftbrowse mcp: {e}");
                    1
                }
            };
            // Sessions are closed by now. Exit directly: the runtime would otherwise
            // wait forever on its blocking stdin reader after a signal.
            std::process::exit(code)
        }
        Command::Capabilities(common) => {
            let steps = vec![("get_capabilities", json!({}))];
            one_shot(&common, steps).await
        }
        Command::Snapshot { url, full, common } => {
            let mode = if full { "full" } else { "compact" };
            one_shot(&common, vec![("navigate", json!({ "url": url })), ("get_page_snapshot", json!({ "mode": mode }))])
                .await
        }
        Command::Screenshot { url, name, full_page, common } => {
            let steps = vec![
                ("navigate", json!({ "url": url })),
                ("get_screenshot", json!({ "name": name, "full_page": full_page })),
            ];
            one_shot(&common, steps).await
        }
        Command::Run { script, mut common } => {
            common.allow_evaluate = true;
            run_script(&script, &common).await
        }
        Command::Test(args) => testrun::run(*args).await,
        Command::Sdk { test_defaults, schema, mut common } => {
            if schema {
                let ops: Vec<Value> = swiftbrowse_core::request::op_specs()
                    .into_iter()
                    .map(|s| json!({ "name": s.name, "description": s.description, "schema": (s.schema)() }))
                    .collect();
                print(&json!({ "protocol": sdk::PROTOCOL, "ops": ops }));
                return ExitCode::SUCCESS;
            }
            if test_defaults {
                common.no_approval = true;
                common.allow_evaluate = true;
                if common.upload_dirs.is_empty() {
                    common.upload_dirs.push(PathBuf::from("."));
                }
            }
            sdk::serve(common).await
        }
    }
}

/// Runs a few operations, printing the last result (or the first failure).
async fn one_shot(common: &Common, steps: Vec<(&str, Value)>) -> ExitCode {
    let mut mgr = common.manager();
    let id = match mgr.create(None, &common.options()).await {
        Ok(info) => info.session_id,
        Err(e) => return fail(e),
    };
    let mut code = ExitCode::SUCCESS;
    let mut last = Value::Null;
    for (op, args) in steps {
        let req = match Request::from_op(op, args) {
            Ok(r) => r,
            Err(e) => {
                mgr.close_all().await;
                return fail(e);
            }
        };
        let r = mgr.execute(&id, req).await;
        last = serde_json::to_value(&r).unwrap_or_default();
        if !r.ok {
            code = ExitCode::FAILURE;
            break;
        }
    }
    mgr.close_all().await;
    print(&last);
    code
}

async fn run_script(path: &PathBuf, common: &Common) -> ExitCode {
    let text = if path.as_os_str() == "-" {
        let mut s = String::new();
        if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s) {
            return fail(e);
        }
        s
    } else {
        match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => return fail(format!("cannot read {}: {e}", path.display())),
        }
    };
    let script: Script = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => return fail(format!("invalid script: {e}")),
    };
    // Validate every step before starting a browser.
    let mut requests = Vec::with_capacity(script.steps.len());
    for (i, step) in script.steps.iter().enumerate() {
        let mut obj = step.as_object().cloned().unwrap_or_default();
        let Some(op) = obj.remove("op").and_then(|v| v.as_str().map(str::to_string)) else {
            return fail(format!("step {i}: missing \"op\""));
        };
        match Request::from_op(&op, Value::Object(obj)) {
            Ok(r) => requests.push(r),
            Err(e) => return fail(format!("step {i}: {}", e.message)),
        }
    }

    let mut mgr = common.manager();
    let id = match mgr.create(None, &common.options()).await {
        Ok(info) => info.session_id,
        Err(e) => return fail(e),
    };
    let mut results = Vec::new();
    let mut failed = false;
    let mut skipped = 0;
    for req in requests {
        if failed && script.stop_on_failure {
            skipped += 1;
            continue;
        }
        let r = mgr.execute(&id, req).await;
        failed |= !r.ok;
        eprintln!("{} {} ({} ms)", if r.ok { "✓" } else { "✗" }, r.op, r.timing.duration_ms);
        results.push(r);
    }
    let report = mgr.execute(&id, Request::GetTestReport(Default::default())).await.data;
    mgr.close_all().await;
    let verdict = report.as_ref().and_then(|r| r["verdict"].as_str()).unwrap_or("failed").to_string();
    print(&json!({
        "name": script.name,
        "ok": !failed,
        "verdict": verdict,
        "skipped_steps": skipped,
        "results": results,
        "report": report,
    }));
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn daemon_command(action: DaemonAction) -> ExitCode {
    use swiftbrowse_chromium::daemon;
    match action {
        DaemonAction::Run { idle_timeout_secs, chromium } => {
            let options = daemon::DaemonOptions {
                executable: chromium.or_else(|| std::env::var_os("SWIFTBROWSE_CHROMIUM").map(PathBuf::from)),
                idle_timeout: std::time::Duration::from_secs(idle_timeout_secs),
                no_sandbox: swiftbrowse_chromium::launch::needs_no_sandbox(),
            };
            match daemon::run(options).await {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("swiftbrowse daemon: {}", e.message);
                    std::process::exit(1)
                }
            }
        }
        DaemonAction::Status => match daemon::call("SwiftBrowse.status").await {
            Some(status) => {
                print(&json!({ "running": true, "endpoint": daemon::endpoint(), "status": status }));
                ExitCode::SUCCESS
            }
            None => {
                print(&json!({ "running": false, "endpoint": daemon::endpoint() }));
                ExitCode::FAILURE
            }
        },
        DaemonAction::Stop => {
            let stopped = daemon::stop(std::time::Duration::from_secs(10)).await;
            print(&json!({ "stopped": stopped }));
            ExitCode::SUCCESS
        }
    }
}
