//! `swiftbrowse test`: discovers flow files, runs their tests in parallel
//! sessions with retries and timeouts, and writes list, JSON, JUnit and HTML
//! reports. A failing test keeps a screenshot and a text snapshot of the page.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Args;
use futures_util::stream::{self, StreamExt};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use swiftbrowse_core::{Request, SessionManager};

use super::Common;

/// Marks a directory this runner created, so it may be cleared on the next run.
const MARKER: &str = ".swiftbrowse-test-results";

#[derive(Args, Clone)]
pub struct TestArgs {
    /// Test files or directories to search for `*.test.json` and `*.spec.json` (default: current directory).
    paths: Vec<PathBuf>,
    /// Tests run at the same time, each in its own isolated session (default: half the CPU cores).
    #[arg(long, short = 'j')]
    workers: Option<usize>,
    /// Retry a failing test this many times; a test that passes on retry is reported as flaky.
    #[arg(long, default_value_t = 0)]
    retries: u32,
    /// Only run tests whose title (`file › test @tag`) matches this regular expression.
    #[arg(long, short = 'g')]
    grep: Option<String>,
    /// Skip tests whose title matches this regular expression.
    #[arg(long)]
    grep_invert: Option<String>,
    /// Run every test this many times (to find flaky tests).
    #[arg(long, default_value_t = 1)]
    repeat_each: usize,
    /// Run one part of the suite, e.g. `2/4`, to split it across CI machines.
    #[arg(long)]
    shard: Option<String>,
    /// Stop starting tests after this many failures (`-x` is `--max-failures 1`).
    #[arg(long)]
    max_failures: Option<usize>,
    #[arg(short = 'x')]
    fail_fast: bool,
    /// Fail if any test is marked `only` (use on CI).
    #[arg(long)]
    forbid_only: bool,
    /// Resolve relative `navigate` URLs against this URL (overrides a file's `base_url`).
    #[arg(long)]
    base_url: Option<String>,
    /// Serve this directory over HTTP on a free local port and use it as the base URL.
    #[arg(long, value_name = "DIR")]
    serve: Option<PathBuf>,
    /// Start this command (e.g. `npm run dev`) before the tests and stop it after;
    /// tests wait until `--web-server-url` accepts connections. An already running
    /// server is reused.
    #[arg(long, value_name = "COMMAND", requires = "web_server_url")]
    web_server: Option<String>,
    #[arg(long, value_name = "URL")]
    web_server_url: Option<String>,
    /// Set a `${name}` variable, overriding the file's `vars` (repeatable).
    #[arg(long = "var", value_name = "NAME=VALUE")]
    vars: Vec<String>,
    /// How long assertions and actions wait for the page (default 5000).
    #[arg(long, default_value_t = 5_000)]
    expect_timeout_ms: u64,
    /// Budget for one test attempt, including hooks (default 30000).
    #[arg(long, default_value_t = 30_000)]
    test_timeout_ms: u64,
    /// Console output: `list` (default), `dot`, `json` (results on stdout) or `quiet`.
    #[arg(long, default_value = "list")]
    reporter: String,
    /// Where screenshots, page snapshots and results.json go.
    #[arg(long, default_value = "test-results")]
    output_dir: PathBuf,
    /// Also write a JUnit XML report here.
    #[arg(long)]
    junit: Option<PathBuf>,
    /// Also write a self-contained HTML report here.
    #[arg(long)]
    html: Option<PathBuf>,
    /// Print the tests that would run, without running them.
    #[arg(long)]
    list: bool,
    #[command(flatten)]
    common: Common,
}

// ---------------------------------------------------------------------------
// Test files

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSpec {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    /// Read before deserializing (see `load`); declared so the field is allowed.
    #[serde(default, rename = "vars")]
    _vars: Map<String, Value>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    before_each: Vec<Value>,
    #[serde(default)]
    after_each: Vec<Value>,
    #[serde(default)]
    tests: Vec<TestSpec>,
    /// A plain `run` script (`name` + `steps`) is a file with one test.
    #[serde(default)]
    steps: Option<Vec<Value>>,
    #[serde(default)]
    stop_on_failure: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestSpec {
    name: String,
    steps: Vec<Value>,
    /// `true`, or a reason string.
    #[serde(default)]
    skip: Value,
    #[serde(default)]
    only: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    retries: Option<u32>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Clone)]
struct Step {
    label: String,
    request: Request,
}

struct TestCase {
    file: String,
    suite: String,
    name: String,
    title: String,
    before: Arc<Vec<Step>>,
    steps: Vec<Step>,
    after: Arc<Vec<Step>>,
    skip: Option<String>,
    only: bool,
    retries: u32,
    timeout: Duration,
}

struct LoadOptions<'a> {
    base_url: Option<&'a str>,
    vars: &'a BTreeMap<String, String>,
    expect_timeout_ms: u64,
    retries: u32,
    test_timeout_ms: u64,
}

fn discover(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    fn is_test(p: &Path) -> bool {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        n.ends_with(".test.json") || n.ends_with(".spec.json")
    }
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type()?.is_dir() {
                if !(name.starts_with('.') || name == "node_modules" || name == "target") {
                    walk(&path, out)?;
                }
            } else if is_test(&path) {
                out.push(path);
            }
        }
        Ok(())
    }
    let roots = if paths.is_empty() { vec![PathBuf::from(".")] } else { paths.to_vec() };
    let mut files = Vec::new();
    for root in roots {
        if root.is_dir() {
            walk(&root, &mut files).map_err(|e| format!("cannot read {}: {e}", root.display()))?;
        } else if root.is_file() {
            files.push(root);
        } else {
            return Err(format!("no such file or directory: {}", root.display()));
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Replaces `${name}` with a variable and `${env.NAME}` with an environment variable.
fn substitute(v: &mut Value, vars: &BTreeMap<String, String>, re: &Regex) -> Result<(), String> {
    match v {
        Value::String(s) if s.contains("${") => {
            let mut missing = None;
            let out = re.replace_all(s, |c: &regex::Captures| {
                let key = &c[1];
                let found = match key.strip_prefix("env.") {
                    Some(env) => std::env::var(env).ok(),
                    None => vars.get(key).cloned(),
                };
                found.unwrap_or_else(|| {
                    missing.get_or_insert_with(|| key.to_string());
                    String::new()
                })
            });
            if let Some(key) = missing {
                return Err(format!("undefined variable ${{{key}}}"));
            }
            *s = out.into_owned();
        }
        Value::Array(items) => {
            for item in items {
                substitute(item, vars, re)?;
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                substitute(item, vars, re)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_step(step: &Value, base: Option<&url::Url>, expect_timeout_ms: u64) -> Result<Step, String> {
    let mut obj = step.as_object().cloned().ok_or("a step must be a JSON object")?;
    let op = obj.remove("op").and_then(|v| v.as_str().map(str::to_string)).ok_or("missing \"op\"")?;
    let label = format!("{op} {}", short(&Value::Object(obj.clone())));
    let op = match op.as_str() {
        // `expect` is an auto-waiting assertion: one `condition` or several `conditions`.
        "expect" => {
            if let Some(c) = obj.remove("condition") {
                obj.insert("conditions".into(), json!([c]));
            }
            "assert_page_state".to_string()
        }
        _ => op,
    };
    if op == "assert_page_state" && !obj.contains_key("timeout_ms") {
        obj.insert("timeout_ms".into(), json!(expect_timeout_ms));
    }
    if op == "navigate" {
        if let (Some(Value::String(u)), Some(base)) = (obj.get("url"), base) {
            if url::Url::parse(u).is_err() {
                let joined = base.join(u).map_err(|e| format!("cannot resolve {u:?} against the base URL: {e}"))?;
                obj.insert("url".into(), json!(joined.as_str()));
            }
        }
    }
    let request = Request::from_op(&op, Value::Object(obj)).map_err(|e| e.message)?;
    Ok(Step { label, request })
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 90 {
        format!("{}…", s.chars().take(90).collect::<String>())
    } else {
        s
    }
}

fn load(path: &Path, opts: &LoadOptions) -> Result<Vec<TestCase>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;
    let mut raw: Value = serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;
    // Variables come first so they can be used anywhere else in the file.
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    if let Some(Value::Object(file_vars)) = raw.get("vars") {
        for (k, v) in file_vars {
            vars.insert(k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
        }
    }
    vars.extend(opts.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
    let re = Regex::new(r"\$\{([A-Za-z0-9_.]+)\}").expect("valid pattern");
    if let Value::Object(map) = &mut raw {
        for (k, v) in map.iter_mut() {
            if k != "vars" {
                substitute(v, &vars, &re)?;
            }
        }
    }
    let spec: FileSpec = serde_json::from_value(raw).map_err(|e| format!("invalid test file: {e}"))?;
    let base = match opts.base_url.or(spec.base_url.as_deref()) {
        Some(b) => Some(url::Url::parse(b).map_err(|e| format!("invalid base_url {b:?}: {e}"))?),
        None => None,
    };
    let file = path.display().to_string();
    let suite = spec.name.clone().unwrap_or_else(|| {
        let n = path.file_name().and_then(|n| n.to_str()).unwrap_or("tests");
        n.trim_end_matches(".json").trim_end_matches(".test").trim_end_matches(".spec").to_string()
    });
    let steps_of = |list: &[Value], what: &str| -> Result<Vec<Step>, String> {
        list.iter()
            .enumerate()
            .map(|(i, s)| {
                parse_step(s, base.as_ref(), opts.expect_timeout_ms).map_err(|e| format!("{what} step {}: {e}", i + 1))
            })
            .collect()
    };
    let before = Arc::new(steps_of(&spec.before_each, "before_each")?);
    let after = Arc::new(steps_of(&spec.after_each, "after_each")?);
    let mut tests = spec.tests;
    if let Some(steps) = spec.steps {
        if !tests.is_empty() {
            return Err("use either top-level \"steps\" or \"tests\", not both".into());
        }
        if spec.stop_on_failure == Some(false) {
            return Err("stop_on_failure=false is not supported by the test runner".into());
        }
        tests.push(TestSpec {
            name: spec.name.clone().unwrap_or_else(|| suite.clone()),
            steps,
            skip: Value::Null,
            only: false,
            tags: Vec::new(),
            retries: None,
            timeout_ms: None,
        });
    }
    let mut out = Vec::new();
    for t in tests {
        let steps = steps_of(&t.steps, &format!("test {:?}", t.name))?;
        let tags: Vec<String> =
            spec.tags.iter().chain(&t.tags).map(|t| format!(" @{}", t.trim_start_matches('@'))).collect();
        let skip = match &t.skip {
            Value::Bool(true) => Some(String::new()),
            Value::String(reason) => Some(reason.clone()),
            Value::Null | Value::Bool(false) => None,
            other => return Err(format!("test {:?}: skip must be true or a reason, not {other}", t.name)),
        };
        out.push(TestCase {
            file: file.clone(),
            title: format!("{suite} › {}{}", t.name, tags.concat()),
            suite: suite.clone(),
            name: t.name,
            before: before.clone(),
            steps,
            after: after.clone(),
            skip,
            only: t.only,
            retries: t.retries.unwrap_or(opts.retries),
            timeout: Duration::from_millis(t.timeout_ms.unwrap_or(opts.test_timeout_ms)),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Running

#[derive(Serialize, Clone)]
struct StepRecord {
    phase: &'static str,
    step: String,
    ok: bool,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// The failed checks, with what was observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    checks: Option<Value>,
    /// What an `evaluate` step returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
}

#[derive(Serialize, Clone, Default)]
struct Attempt {
    ok: bool,
    duration_ms: u64,
    steps: Vec<StepRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    screenshot: Option<String>,
    /// Text snapshot of the page when the test failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    console_errors: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    network_failures: Vec<Value>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Passed,
    Failed,
    Flaky,
    Skipped,
}

#[derive(Serialize, Clone)]
struct TestResult {
    file: String,
    suite: String,
    name: String,
    title: String,
    status: Status,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<String>,
    attempts: Vec<Attempt>,
}

struct Runner {
    common: Common,
    expect_timeout_ms: u64,
    timeout_ms: Option<u64>,
    output_dir: PathBuf,
    failures: AtomicUsize,
    max_failures: Option<usize>,
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(60).collect()
}

impl Runner {
    fn manager(&self, dir: &Path) -> SessionManager {
        let mut policy = self.common.policy();
        policy.evidence_dir = dir.to_path_buf();
        // The runner captures one screenshot per failed attempt itself.
        policy.screenshot_on_failure = false;
        policy.allow_evaluate = true;
        // Tests run against test environments: "Delete" and "Pay" are normal clicks.
        policy.require_approval_for_consequential = false;
        policy.default_timeout_ms = self.timeout_ms.unwrap_or(self.expect_timeout_ms);
        self.common.manager_with(policy)
    }

    async fn run_test(&self, index: usize, case: &TestCase) -> TestResult {
        let mut result = TestResult {
            file: case.file.clone(),
            suite: case.suite.clone(),
            name: case.name.clone(),
            title: case.title.clone(),
            status: Status::Skipped,
            duration_ms: 0,
            skip_reason: case.skip.clone(),
            attempts: Vec::new(),
        };
        if case.skip.is_some() {
            return result;
        }
        let started = Instant::now();
        for n in 0..=case.retries {
            if self.max_failures.is_some_and(|m| self.failures.load(Ordering::SeqCst) >= m) {
                if result.attempts.is_empty() {
                    result.skip_reason = Some("not run: maximum failures reached".into());
                    return result;
                }
                break;
            }
            let mut dir = self.output_dir.join(format!("{:03}-{}", index + 1, slug(&case.title)));
            if n > 0 {
                dir = dir.join(format!("retry{n}"));
            }
            let attempt = self.attempt(case, &dir).await;
            let ok = attempt.ok;
            result.attempts.push(attempt);
            if ok {
                break;
            }
        }
        result.duration_ms = started.elapsed().as_millis() as u64;
        let last_ok = result.attempts.last().is_some_and(|a| a.ok);
        result.status = match (last_ok, result.attempts.len()) {
            (true, 1) => Status::Passed,
            (true, _) => Status::Flaky,
            (false, _) => Status::Failed,
        };
        if result.status == Status::Failed {
            self.failures.fetch_add(1, Ordering::SeqCst);
        }
        result
    }

    async fn attempt(&self, case: &TestCase, dir: &Path) -> Attempt {
        let started = Instant::now();
        let mut out = Attempt::default();
        let mut mgr = self.manager(dir);
        let id = match mgr.create(None, &self.common.options()).await {
            Ok(info) => info.session_id,
            Err(e) => {
                out.error = Some(format!("could not start a browser session: {}", e.message));
                out.duration_ms = started.elapsed().as_millis() as u64;
                return out;
            }
        };
        let mut records = Vec::new();
        let body = async {
            for (phase, list) in [("before_each", &case.before[..]), ("test", &case.steps[..])] {
                for step in list {
                    if !run_step(&mut mgr, &id, phase, step, &mut records).await {
                        return false;
                    }
                }
            }
            true
        };
        let mut ok = match tokio::time::timeout(case.timeout, body).await {
            Ok(ok) => ok,
            Err(_) => {
                out.error = Some(format!("test timeout of {} ms exceeded", case.timeout.as_millis()));
                false
            }
        };
        // `after_each` always runs, with its own budget.
        if !case.after.is_empty() {
            let budget = Duration::from_millis(self.expect_timeout_ms.max(10_000));
            let teardown = async {
                let mut all = true;
                for step in case.after.iter() {
                    all &= run_step(&mut mgr, &id, "after_each", step, &mut records).await;
                }
                all
            };
            ok &= tokio::time::timeout(budget, teardown).await.unwrap_or(false);
        }
        if !ok {
            if out.error.is_none() {
                out.error = records.iter().find(|r| !r.ok).and_then(|r| r.error.clone());
            }
            self.collect_failure(&mut mgr, &id, dir, &mut out).await;
        }
        mgr.close_all().await;
        out.ok = ok;
        out.steps = records;
        out.duration_ms = started.elapsed().as_millis() as u64;
        out
    }

    /// Screenshot, page snapshot and diagnostics for a failed attempt.
    async fn collect_failure(&self, mgr: &mut SessionManager, id: &str, dir: &Path, out: &mut Attempt) {
        let budget = Duration::from_secs(5);
        let shot = Request::from_op("get_screenshot", json!({ "name": "failure" })).expect("valid request");
        if let Ok(r) = tokio::time::timeout(budget, mgr.execute(id, shot)).await {
            out.screenshot = r.data.as_ref().and_then(|d| d["path"].as_str()).map(str::to_string);
        }
        let snap = Request::from_op("get_page_snapshot", json!({ "format": "text" })).expect("valid request");
        if let Ok(r) = tokio::time::timeout(budget, mgr.execute(id, snap)).await {
            if let Some(text) = r.data.as_ref().and_then(|d| d["text"].as_str()) {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join("failure-page.txt"), text);
                out.page = Some(text.to_string());
            }
        }
        for (op, field) in [("get_console_errors", "errors"), ("get_network_failures", "failures")] {
            let req = Request::from_op(op, json!({})).expect("valid request");
            if let Ok(r) = tokio::time::timeout(budget, mgr.execute(id, req)).await {
                let items = r.data.as_ref().and_then(|d| d[field].as_array().cloned()).unwrap_or_default();
                let items: Vec<Value> = items.into_iter().take(5).collect();
                if op == "get_console_errors" {
                    out.console_errors = items;
                } else {
                    out.network_failures = items;
                }
            }
        }
    }
}

async fn run_step(
    mgr: &mut SessionManager,
    id: &str,
    phase: &'static str,
    step: &Step,
    records: &mut Vec<StepRecord>,
) -> bool {
    let r = mgr.execute(id, step.request.clone()).await;
    let checks = r.verification.as_ref().filter(|_| !r.ok).map(|v| {
        let failed: Vec<_> = v.checks.iter().filter(|c| !c.passed).collect();
        serde_json::to_value(failed).unwrap_or_default()
    });
    let error = r.error.as_ref().map(|e| {
        let mut msg = e.message.clone();
        if let Some(note) = r.notes.first() {
            msg = format!("{msg} ({note})");
        }
        msg
    });
    let value = match step.request {
        Request::Evaluate(_) => r.data.as_ref().map(|d| d["value"].clone()),
        _ => None,
    };
    records.push(StepRecord {
        phase,
        step: step.label.clone(),
        ok: r.ok,
        duration_ms: r.timing.duration_ms,
        error,
        checks,
        value,
    });
    r.ok
}

// ---------------------------------------------------------------------------
// Entry point and reports

fn parse_shard(s: &str) -> Result<(usize, usize), String> {
    let (i, n) = s.split_once('/').ok_or("shard must look like 1/3")?;
    let (i, n): (usize, usize) = (i.parse().map_err(|_| "bad shard index")?, n.parse().map_err(|_| "bad shard count")?);
    if n == 0 || i == 0 || i > n {
        return Err("shard must be i/n with 1 <= i <= n".into());
    }
    Ok((i, n))
}

fn config_error(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("swiftbrowse test: {msg}");
    ExitCode::from(2)
}

/// Serves `root` on 127.0.0.1 and returns its base URL. Read-only, GET/HEAD only,
/// never outside `root`.
async fn serve_dir(root: PathBuf) -> std::io::Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = std::fs::canonicalize(root)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let root = root.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut len = 0;
                while !buf[..len].windows(4).any(|w| w == b"\r\n\r\n") && len < buf.len() {
                    match stream.read(&mut buf[len..]).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => len += n,
                    }
                }
                let head = String::from_utf8_lossy(&buf[..len]).to_string();
                let mut parts = head.split_whitespace();
                let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
                let path = target.split(['?', '#']).next().unwrap_or("/");
                let path = percent_decode(path);
                let mut file = root.join(path.trim_start_matches('/'));
                if file.is_dir() {
                    file = file.join("index.html");
                }
                let inside = std::fs::canonicalize(&file).ok().filter(|f| f.starts_with(&root) && f.is_file());
                let (status, body, ctype) = match (method, inside) {
                    ("GET" | "HEAD", Some(f)) => match std::fs::read(&f) {
                        Ok(b) => ("200 OK", b, content_type(&f)),
                        Err(_) => ("404 Not Found", b"not found".to_vec(), "text/plain"),
                    },
                    ("GET" | "HEAD", None) => ("404 Not Found", b"not found".to_vec(), "text/plain"),
                    _ => ("405 Method Not Allowed", Vec::new(), "text/plain"),
                };
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                if method != "HEAD" {
                    let _ = stream.write_all(&body).await;
                }
                let _ = stream.shutdown().await;
            });
        }
    });
    Ok(format!("http://{addr}/"))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or_default() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Starts `command` unless `url` already accepts connections, then waits until it does.
async fn start_web_server(command: &str, url: &str) -> Result<Option<tokio::process::Child>, String> {
    let u = url::Url::parse(url).map_err(|e| format!("invalid --web-server-url: {e}"))?;
    let host = u.host_str().unwrap_or("127.0.0.1").to_string();
    let port = u.port_or_known_default().unwrap_or(80);
    let up = || {
        let (host, port) = (host.clone(), port);
        async move { tokio::net::TcpStream::connect((host.as_str(), port)).await.is_ok() }
    };
    if up().await {
        return Ok(None);
    }
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    let mut child = cmd
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("cannot start web server {command:?}: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if up().await {
            return Ok(Some(child));
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("web server {command:?} exited with {status} before {url} was reachable"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("web server did not answer on {url} within 120 s"))
}

pub async fn run(args: TestArgs) -> ExitCode {
    let mut base_url = args.base_url.clone();
    if let Some(dir) = &args.serve {
        match serve_dir(dir.clone()).await {
            Ok(url) => {
                base_url.get_or_insert(url);
            }
            Err(e) => return config_error(format!("cannot serve {}: {e}", dir.display())),
        }
    }
    // Held until the end of the run; dropping it stops the server.
    let _web_server = match (&args.web_server, &args.web_server_url) {
        (Some(cmd), Some(url)) => match start_web_server(cmd, url).await {
            Ok(child) => {
                base_url.get_or_insert(url.clone());
                child
            }
            Err(e) => return config_error(e),
        },
        _ => None,
    };
    let files = match discover(&args.paths) {
        Ok(f) if f.is_empty() => return config_error("no test files found (*.test.json or *.spec.json)"),
        Ok(f) => f,
        Err(e) => return config_error(e),
    };
    let mut vars = BTreeMap::new();
    for v in &args.vars {
        match v.split_once('=') {
            Some((k, val)) => vars.insert(k.to_string(), val.to_string()),
            None => return config_error(format!("--var needs NAME=VALUE, got {v:?}")),
        };
    }
    let opts = LoadOptions {
        base_url: base_url.as_deref(),
        vars: &vars,
        expect_timeout_ms: args.expect_timeout_ms,
        retries: args.retries,
        test_timeout_ms: args.test_timeout_ms,
    };
    let mut cases = Vec::new();
    for f in &files {
        match load(f, &opts) {
            Ok(c) => cases.extend(c),
            Err(e) => return config_error(format!("{}: {e}", f.display())),
        }
    }
    if cases.iter().any(|c| c.only) {
        if args.forbid_only {
            let titles: Vec<&str> = cases.iter().filter(|c| c.only).map(|c| c.title.as_str()).collect();
            return config_error(format!("tests marked only with --forbid-only: {}", titles.join(", ")));
        }
        cases.retain(|c| c.only);
    }
    let compile = |p: &Option<String>| p.as_deref().map(Regex::new).transpose();
    let (grep, grep_invert) = match (compile(&args.grep), compile(&args.grep_invert)) {
        (Ok(g), Ok(v)) => (g, v),
        (Err(e), _) | (_, Err(e)) => return config_error(format!("bad --grep pattern: {e}")),
    };
    cases.retain(|c| grep.as_ref().is_none_or(|g| g.is_match(&c.title)));
    cases.retain(|c| grep_invert.as_ref().is_none_or(|g| !g.is_match(&c.title)));
    if let Some(s) = &args.shard {
        let (i, n) = match parse_shard(s) {
            Ok(x) => x,
            Err(e) => return config_error(e),
        };
        cases = cases.into_iter().enumerate().filter(|(k, _)| k % n == i - 1).map(|(_, c)| c).collect();
    }
    if args.repeat_each > 1 {
        let n = args.repeat_each;
        let mut repeated = Vec::with_capacity(cases.len() * n);
        for c in cases {
            for k in 1..n {
                repeated.push(TestCase {
                    title: format!("{} (repeat {})", c.title, k + 1),
                    before: c.before.clone(),
                    steps: c.steps.clone(),
                    after: c.after.clone(),
                    file: c.file.clone(),
                    suite: c.suite.clone(),
                    name: c.name.clone(),
                    skip: c.skip.clone(),
                    only: c.only,
                    retries: c.retries,
                    timeout: c.timeout,
                });
            }
            repeated.insert(repeated.len() + 1 - n, c);
        }
        cases = repeated;
    }
    if args.list {
        for c in &cases {
            println!("{}{}", c.title, if c.skip.is_some() { " (skipped)" } else { "" });
        }
        println!("{} tests in {} files", cases.len(), files.len());
        return ExitCode::SUCCESS;
    }
    if cases.is_empty() {
        return config_error("no tests matched");
    }

    // Clear results from an earlier run, but only in a directory this runner created.
    if args.output_dir.join(MARKER).exists() {
        let _ = std::fs::remove_dir_all(&args.output_dir);
    }
    if let Err(e) = std::fs::create_dir_all(&args.output_dir) {
        return config_error(format!("cannot create {}: {e}", args.output_dir.display()));
    }
    let _ = std::fs::write(args.output_dir.join(MARKER), "");
    let output_dir = std::fs::canonicalize(&args.output_dir).unwrap_or(args.output_dir.clone());

    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let workers = args.workers.unwrap_or((cpus / 2).max(1)).max(1);
    let runner = Arc::new(Runner {
        common: args.common.clone(),
        expect_timeout_ms: args.expect_timeout_ms,
        timeout_ms: args.common.timeout_ms,
        output_dir: output_dir.clone(),
        failures: AtomicUsize::new(0),
        max_failures: if args.fail_fast { Some(1) } else { args.max_failures },
    });
    let reporter = args.reporter.as_str();
    if reporter == "list" {
        let s = if cases.len() == 1 { "" } else { "s" };
        println!(
            "\nRunning {} test{s} using {} worker{}\n",
            cases.len(),
            workers.min(cases.len()),
            if workers == 1 { "" } else { "s" }
        );
    }
    let started = Instant::now();
    let cases = Arc::new(cases);
    let mut results: Vec<(usize, TestResult)> = stream::iter(0..cases.len())
        .map(|i| {
            let (runner, cases) = (runner.clone(), cases.clone());
            async move {
                let r = runner.run_test(i, &cases[i]).await;
                print_progress(reporter, &r);
                (i, r)
            }
        })
        .buffer_unordered(workers)
        .collect()
        .await;
    results.sort_by_key(|(i, _)| *i);
    let results: Vec<TestResult> = results.into_iter().map(|(_, r)| r).collect();
    let wall_ms = started.elapsed().as_millis() as u64;

    let count = |s: Status| results.iter().filter(|r| r.status == s).count();
    let (passed, failed, flaky, skipped) =
        (count(Status::Passed), count(Status::Failed), count(Status::Flaky), count(Status::Skipped));
    let summary = json!({
        "ok": failed == 0,
        "passed": passed, "failed": failed, "flaky": flaky, "skipped": skipped,
        "duration_ms": wall_ms, "workers": workers,
        "output_dir": output_dir.display().to_string(),
    });
    let doc = json!({ "summary": summary, "tests": results });
    let _ = std::fs::write(output_dir.join("results.json"), serde_json::to_string_pretty(&doc).unwrap_or_default());
    if let Some(p) = &args.junit {
        if let Err(e) = std::fs::write(p, junit(&results, wall_ms)) {
            eprintln!("cannot write {}: {e}", p.display());
        }
    }
    if let Some(p) = &args.html {
        if let Err(e) = std::fs::write(p, html(&results, &summary)) {
            eprintln!("cannot write {}: {e}", p.display());
        }
    }
    match reporter {
        "json" => println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default()),
        "quiet" => {}
        _ => {
            if reporter == "dot" {
                println!();
            }
            print_failures(&results);
            let mut parts = Vec::new();
            for (n, what) in [(failed, "failed"), (flaky, "flaky"), (passed, "passed"), (skipped, "skipped")] {
                if n > 0 {
                    parts.push(format!("{n} {what}"));
                }
            }
            println!("\n  {} ({:.1}s)", parts.join(", "), wall_ms as f64 / 1000.0);
            if let Some(p) = &args.html {
                println!("  HTML report: {}", p.display());
            }
        }
    }
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_progress(reporter: &str, r: &TestResult) {
    use std::io::Write;
    match reporter {
        "list" => {
            let (mark, extra) = match r.status {
                Status::Passed => ("✓", String::new()),
                Status::Flaky => ("±", format!(" flaky, passed on try {}", r.attempts.len())),
                Status::Failed => ("✘", String::new()),
                Status::Skipped => {
                    ("-", r.skip_reason.clone().filter(|s| !s.is_empty()).map(|s| format!(" {s}")).unwrap_or_default())
                }
            };
            let time = if r.status == Status::Skipped { String::new() } else { format!(" ({} ms)", r.duration_ms) };
            println!("  {mark} {}{time}{extra}", r.title);
        }
        "dot" => {
            let c = match r.status {
                Status::Passed => "·",
                Status::Flaky => "±",
                Status::Failed => "F",
                Status::Skipped => "°",
            };
            print!("{c}");
            let _ = std::io::stdout().flush();
        }
        _ => {}
    }
}

fn print_failures(results: &[TestResult]) {
    let failed: Vec<&TestResult> =
        results.iter().filter(|r| matches!(r.status, Status::Failed | Status::Flaky)).collect();
    for (n, r) in failed.iter().enumerate() {
        let Some(a) = r.attempts.iter().rev().find(|a| !a.ok) else { continue };
        let tag = if r.status == Status::Flaky { " (flaky)" } else { "" };
        println!("\n  {}) {}{tag}  [{}]", n + 1, r.title, r.file);
        if let Some(s) = a.steps.iter().find(|s| !s.ok) {
            println!("     ✘ {} step: {}", s.phase, s.step);
        }
        if let Some(e) = &a.error {
            println!("     {e}");
        }
        if let Some(s) = a.steps.iter().find(|s| !s.ok) {
            if let Some(Value::Array(checks)) = &s.checks {
                for c in checks {
                    println!("     expected {}  observed {}", short(&c["condition"]), short(&c["observed"]));
                }
            }
        }
        for e in &a.console_errors {
            println!("     console: {}", short(e.get("text").unwrap_or(e)));
        }
        for f in &a.network_failures {
            println!("     network: {}", short(f));
        }
        if let Some(p) = &a.screenshot {
            println!("     screenshot: {p}");
        }
        if let Some(page) = &a.page {
            println!("     page at failure:");
            let lines: Vec<&str> = page.lines().collect();
            for l in lines.iter().take(20) {
                println!("       {l}");
            }
            if lines.len() > 20 {
                println!("       … {} more lines", lines.len() - 20);
            }
        }
    }
}

fn xml(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

fn failure_text(a: &Attempt) -> String {
    let mut out = String::new();
    for s in &a.steps {
        out.push_str(&format!(
            "{} {} [{}] {} ({} ms)\n",
            if s.ok { "✓" } else { "✘" },
            s.phase,
            s.step,
            s.error.clone().unwrap_or_default(),
            s.duration_ms
        ));
    }
    if let Some(e) = &a.error {
        out.push_str(&format!("\n{e}\n"));
    }
    if let Some(p) = &a.screenshot {
        out.push_str(&format!("screenshot: {p}\n"));
    }
    if let Some(page) = &a.page {
        out.push_str(&format!("\npage at failure:\n{page}\n"));
    }
    out
}

fn junit(results: &[TestResult], wall_ms: u64) -> String {
    let mut by_file: BTreeMap<&str, Vec<&TestResult>> = BTreeMap::new();
    for r in results {
        by_file.entry(&r.file).or_default().push(r);
    }
    let count = |rs: &[&TestResult], s: Status| rs.iter().filter(|r| r.status == s).count();
    let all: Vec<&TestResult> = results.iter().collect();
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"swiftbrowse\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"0\" time=\"{:.3}\">\n",
        results.len(),
        count(&all, Status::Failed),
        count(&all, Status::Skipped),
        wall_ms as f64 / 1000.0
    );
    for (file, rs) in by_file {
        let time: u64 = rs.iter().map(|r| r.duration_ms).sum();
        out.push_str(&format!(
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"0\" time=\"{:.3}\">\n",
            xml(file),
            rs.len(),
            count(&rs, Status::Failed),
            count(&rs, Status::Skipped),
            time as f64 / 1000.0
        ));
        for r in rs {
            out.push_str(&format!(
                "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\"",
                xml(&r.suite),
                xml(&r.name),
                r.duration_ms as f64 / 1000.0
            ));
            let failed_attempt = r.attempts.iter().rev().find(|a| !a.ok);
            match (r.status, failed_attempt) {
                (Status::Skipped, _) => out.push_str(&format!(
                    ">\n      <skipped message=\"{}\"/>\n    </testcase>\n",
                    xml(r.skip_reason.as_deref().unwrap_or(""))
                )),
                (Status::Failed, Some(a)) => out.push_str(&format!(
                    ">\n      <failure message=\"{}\" type=\"failure\">{}</failure>\n    </testcase>\n",
                    xml(a.error.as_deref().unwrap_or("failed")),
                    xml(&failure_text(a))
                )),
                (Status::Flaky, Some(a)) => out.push_str(&format!(
                    ">\n      <system-out>flaky: passed on try {}\n{}</system-out>\n    </testcase>\n",
                    r.attempts.len(),
                    xml(&failure_text(a))
                )),
                _ => out.push_str("/>\n"),
            }
        }
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

fn html(results: &[TestResult], summary: &Value) -> String {
    use base64::Engine;
    let esc = |s: &str| xml(s);
    let mut rows = String::new();
    for r in results {
        let (cls, label) = match r.status {
            Status::Passed => ("pass", "passed"),
            Status::Failed => ("fail", "failed"),
            Status::Flaky => ("flaky", "flaky"),
            Status::Skipped => ("skip", "skipped"),
        };
        let mut body = String::new();
        for (n, a) in r.attempts.iter().enumerate() {
            if r.attempts.len() > 1 {
                body.push_str(&format!("<h4>Try {} · {}</h4>", n + 1, if a.ok { "passed" } else { "failed" }));
            }
            body.push_str("<ol class=steps>");
            for s in &a.steps {
                body.push_str(&format!(
                    "<li class={}><span class=mark>{}</span><code>{}</code><span class=ms>{} ms</span>{}</li>",
                    if s.ok { "ok" } else { "bad" },
                    if s.ok { "✓" } else { "✘" },
                    esc(&s.step),
                    s.duration_ms,
                    s.error.as_deref().map(|e| format!("<div class=err>{}</div>", esc(e))).unwrap_or_default()
                ));
            }
            body.push_str("</ol>");
            if let (Some(e), false) = (&a.error, a.ok) {
                body.push_str(&format!("<p class=err>{}</p>", esc(e)));
            }
            if let Some(p) = &a.screenshot {
                if let Ok(bytes) = std::fs::read(p) {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                    body.push_str(&format!("<img alt=\"Page at failure\" src=\"data:image/png;base64,{b64}\">"));
                }
            }
            if let Some(page) = &a.page {
                body.push_str(&format!(
                    "<details><summary>Page at failure</summary><pre>{}</pre></details>",
                    esc(page)
                ));
            }
        }
        if let Some(reason) = r.skip_reason.as_deref().filter(|s| !s.is_empty()) {
            body.push_str(&format!("<p>{}</p>", esc(reason)));
        }
        let open = if r.status == Status::Failed { " open" } else { "" };
        rows.push_str(&format!(
            "<details class=\"test {cls}\"{open}><summary><span class=badge>{label}</span> {} <span class=ms>{} ms</span></summary><p class=file>{}</p>{body}</details>\n",
            esc(&r.title),
            r.duration_ms,
            esc(&r.file)
        ));
    }
    let n = |k: &str| summary[k].as_u64().unwrap_or(0);
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>SwiftBrowse test report</title>
<style>
:root {{ --bg:#fafafa; --fg:#1a1a1a; --muted:#666; --card:#fff; --line:#e3e3e3; --pass:#1a7f37; --fail:#cf222e; --flaky:#9a6700; --skip:#6e7781; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#0d1117; --fg:#e6edf3; --muted:#8b949e; --card:#161b22; --line:#30363d; --pass:#3fb950; --fail:#f85149; --flaky:#d29922; --skip:#8b949e; }} }}
body {{ background:var(--bg); color:var(--fg); font:14px/1.5 system-ui,sans-serif; margin:0; padding:24px 16px; }}
main {{ max-width:960px; margin:0 auto; }}
h1 {{ font-size:20px; margin:0 0 4px; }}
.summary {{ color:var(--muted); margin-bottom:20px; }}
.summary b.pass {{ color:var(--pass); }} .summary b.fail {{ color:var(--fail); }} .summary b.flaky {{ color:var(--flaky); }}
.test {{ background:var(--card); border:1px solid var(--line); border-left:4px solid var(--skip); border-radius:6px; margin:8px 0; padding:8px 12px; }}
.test.pass {{ border-left-color:var(--pass); }} .test.fail {{ border-left-color:var(--fail); }} .test.flaky {{ border-left-color:var(--flaky); }}
summary {{ cursor:pointer; }}
.badge {{ display:inline-block; min-width:56px; font-size:12px; font-weight:600; }}
.pass .badge {{ color:var(--pass); }} .fail .badge {{ color:var(--fail); }} .flaky .badge {{ color:var(--flaky); }} .skip .badge {{ color:var(--skip); }}
.ms, .file {{ color:var(--muted); font-size:12px; }}
.steps {{ padding-left:20px; }} .steps li {{ margin:2px 0; }} .steps .mark {{ display:inline-block; width:18px; }}
.steps .bad .mark, .err {{ color:var(--fail); }} .steps .ok .mark {{ color:var(--pass); }}
code {{ font:12px ui-monospace,monospace; overflow-wrap:anywhere; }}
.steps .ms {{ margin-left:8px; }}
pre {{ font:12px ui-monospace,monospace; background:var(--bg); padding:8px; overflow-x:auto; }}
img {{ max-width:100%; border:1px solid var(--line); margin:8px 0; }}
</style></head><body><main>
<h1>Test report</h1>
<p class="summary"><b class="pass">{} passed</b> · <b class="fail">{} failed</b> · <b class="flaky">{} flaky</b> · {} skipped · {:.1}s with {} workers</p>
{rows}</main></body></html>
"#,
        n("passed"),
        n("failed"),
        n("flaky"),
        n("skipped"),
        n("duration_ms") as f64 / 1000.0,
        n("workers"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_relative_urls_and_expect_sugar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("login.test.json");
        std::fs::write(
            &path,
            r#"{ "base_url": "http://localhost:3000/app/", "vars": { "user": "ada" }, "tags": ["smoke"],
                 "tests": [
                   { "name": "a", "steps": [ { "op": "navigate", "url": "login?u=${user}" },
                                             { "op": "expect", "condition": { "type": "text_visible", "text": "Hi ${user}" } } ] },
                   { "name": "b", "skip": "flaky upstream", "steps": [] } ] }"#,
        )
        .unwrap();
        let vars = BTreeMap::new();
        let opts =
            LoadOptions { base_url: None, vars: &vars, expect_timeout_ms: 1234, retries: 2, test_timeout_ms: 9000 };
        let cases = load(&path, &opts).unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].title, "login › a @smoke");
        let nav = serde_json::to_value(&cases[0].steps[0].request).unwrap();
        assert_eq!(nav["url"], "http://localhost:3000/app/login?u=ada");
        let exp = serde_json::to_value(&cases[0].steps[1].request).unwrap();
        assert_eq!(exp["op"], "assert_page_state");
        assert_eq!(exp["timeout_ms"], 1234);
        assert_eq!(exp["conditions"][0]["text"], "Hi ada");
        assert_eq!(cases[0].retries, 2);
        assert_eq!(cases[1].skip.as_deref(), Some("flaky upstream"));

        std::fs::write(
            &path,
            r#"{ "tests": [ { "name": "x", "steps": [ { "op": "navigate", "url": "${nope}" } ] } ] }"#,
        )
        .unwrap();
        assert!(load_err(&path, &opts).contains("undefined variable ${nope}"));
        std::fs::write(&path, r#"{ "tests": [ { "name": "x", "steps": [ { "op": "clik" } ] } ] }"#).unwrap();
        assert!(load_err(&path, &opts).contains("test \"x\" step 1"));
    }

    fn load_err(path: &Path, opts: &LoadOptions) -> String {
        match load(path, opts) {
            Err(e) => e,
            Ok(_) => panic!("expected a load error"),
        }
    }

    #[test]
    fn shards_and_slugs() {
        assert_eq!(parse_shard("2/3").unwrap(), (2, 3));
        assert!(parse_shard("4/3").is_err());
        assert_eq!(slug("Login › signs in @smoke"), "login-signs-in-smoke");
    }

    #[test]
    fn junit_escapes_and_counts() {
        let r = TestResult {
            file: "a<b>.test.json".into(),
            suite: "s".into(),
            name: "t \"1\"".into(),
            title: "s › t".into(),
            status: Status::Failed,
            duration_ms: 12,
            skip_reason: None,
            attempts: vec![Attempt { error: Some("boom & bust".into()), ..Default::default() }],
        };
        let x = junit(&[r], 20);
        assert!(x.contains("failures=\"1\""));
        assert!(x.contains("name=\"t &quot;1&quot;\""));
        assert!(x.contains("message=\"boom &amp; bust\""));
        assert!(x.contains("a&lt;b&gt;.test.json"));
    }
}
