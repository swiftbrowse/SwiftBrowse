//! Chromium backend: one browser process and one isolated browser context per
//! session, driven over the Chrome DevTools Protocol.

mod cdp;
pub mod daemon;
mod keys;
pub mod launch;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use swiftbrowse_core::capability::{cap, names};
use swiftbrowse_core::model::*;
use swiftbrowse_core::request::{SnapshotMode, WaitUntil};
use swiftbrowse_core::{
    Backend, BackendFactory, CapabilityReport, CoreError, ErrorKind, Locator, Result, SessionOptions, Support,
};
use tokio::sync::watch;

use cdp::{Cdp, Event};

const HELPER: &str = include_str!("helper.js");
const CMD_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BUFFER: usize = 200;
const WORLD: &str = "swiftbrowse";
const HELPER_MISSING: &str = "__swiftbrowse_helper_missing__";
/// Longest wait for a client-rendered page that is still empty at `load`.
const RENDER_WAIT: Duration = Duration::from_secs(5);

/// What the page reports when navigation completes in the page.
#[derive(Default, serde::Deserialize)]
struct Loaded {
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    rendered_after_load_ms: Option<u64>,
    #[serde(default)]
    still_empty: bool,
    #[serde(default)]
    still_changing: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ChromiumFactory {
    /// Browser binary; discovered with [`launch::discover`] when `None`.
    pub executable: Option<PathBuf>,
    /// Force `--no-sandbox` on or off; `None` means only when running as root.
    pub no_sandbox: Option<bool>,
    pub extra_args: Vec<String>,
    /// Attach headless sessions to the warm-browser daemon.
    pub daemon: DaemonMode,
}

#[derive(Debug, Clone, Default)]
pub enum DaemonMode {
    /// Every session launches its own browser process.
    #[default]
    Off,
    /// Attach to the warm-browser daemon; start it with this command when none
    /// is running. Falls back to launching a browser if the daemon is unavailable.
    Auto { program: PathBuf, args: Vec<String> },
}

#[async_trait]
impl BackendFactory for ChromiumFactory {
    fn name(&self) -> &str {
        "chromium"
    }

    async fn launch(&self, options: &SessionOptions) -> Result<Box<dyn Backend>> {
        if let (DaemonMode::Auto { program, args }, true) = (&self.daemon, options.headless) {
            let spawn = (program.clone(), args.clone());
            if let Some((r, w)) = daemon::connect_or_spawn(Some(&spawn)).await {
                match ChromiumBackend::attach_daemon(r, w, options).await {
                    Ok(backend) => return Ok(Box::new(backend)),
                    Err(e) => trace_event(&format!("daemon attach failed, launching instead: {}", e.message)),
                }
            }
        }
        let exe = self.executable.clone().or_else(|| {
            let shell = if options.headless { launch::discover_headless_shell() } else { None };
            shell.or_else(launch::discover)
        });
        let exe = exe.ok_or_else(|| {
            CoreError::new(
                ErrorKind::Unsupported,
                format!("no Chromium found; set {} or run `swiftbrowse doctor`", launch::ENV_EXECUTABLE),
            )
        })?;
        Ok(Box::new(
            ChromiumBackend::start(
                &exe,
                options,
                self.no_sandbox.unwrap_or_else(launch::needs_no_sandbox),
                &self.extra_args,
            )
            .await?,
        ))
    }
}

#[derive(Default)]
struct State {
    main_frame: String,
    world: Option<i64>,
    /// Isolated worlds created for the main frame so far; tells navigation when the new document's world exists.
    worlds_created: u64,
    /// Loader id of the newest main-frame document (lifecycle `init`).
    last_init_loader: String,
    /// Loader id of the document the current isolated world belongs to.
    world_loader: String,
    helper_in: Option<i64>,
    lifecycle: HashSet<(String, String)>,
    doc_status: HashMap<String, u16>,
    requests: HashMap<String, (String, String)>,
    /// In-flight scripts and data requests: what a client-rendered page still needs before it can render.
    inflight: HashMap<String, String>,
    console: VecDeque<ConsoleEntry>,
    network: VecDeque<NetworkFailure>,
    dialogs: VecDeque<DialogEvent>,
    dialogs_reported: usize,
}

pub struct ChromiumBackend {
    cdp: Cdp,
    session: String,
    state: Arc<Mutex<State>>,
    lifecycle: watch::Receiver<u64>,
    /// The browser process, when this session owns one (not when attached to the daemon).
    child: Option<tokio::process::Child>,
    profile: Option<tempfile::TempDir>,
    /// The browser context this session owns inside the shared daemon browser.
    daemon_context: Option<String>,
    pump: tokio::task::JoinHandle<()>,
    version: Option<String>,
    headless_shell: bool,
    /// Document generation when the last click was dispatched.
    worlds_at_click: u64,
    headless: bool,
    no_sandbox: bool,
}

fn push_bounded<T>(q: &mut VecDeque<T>, item: T) {
    if q.len() >= MAX_BUFFER {
        q.pop_front();
    }
    q.push_back(item);
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

impl ChromiumBackend {
    pub async fn start(exe: &Path, options: &SessionOptions, no_sandbox: bool, extra: &[String]) -> Result<Self> {
        let t0 = Instant::now();
        let trace = |phase: &str| trace(t0, phase);
        let launched = launch::launch(exe, options.headless, no_sandbox, extra).await?;
        trace("browser process ready");
        let (cdp, events) = match launched.transport {
            launch::Transport::WebSocket(ref url) => Cdp::connect(url).await?,
            #[cfg(unix)]
            launch::Transport::Pipe(to_browser, from_browser) => Cdp::over_stream(to_browser, from_browser),
        };
        let stderr_tail = launched.stderr_tail.clone();
        let startup_error = move |e: CoreError| {
            let tail = stderr_tail.lock().unwrap().join("\n");
            if tail.is_empty() {
                e
            } else {
                CoreError::backend(format!("{}; browser output:\n{tail}", e.message))
            }
        };
        trace("devtools connected");

        // Each session already owns a browser process and a throwaway profile, so
        // the initial tab is isolated; reuse it rather than creating a context.
        let (version, targets) = tokio::join!(
            cdp.send("Browser.getVersion", json!({}), None, CMD_TIMEOUT),
            cdp.send("Target.getTargets", json!({}), None, CMD_TIMEOUT),
        );
        let version = version.ok().map(|v| s(&v, "product"));
        let existing = targets.map_err(&startup_error)?["targetInfos"]
            .as_array()
            .and_then(|t| t.iter().find(|t| s(t, "type") == "page").map(|t| s(t, "targetId")));
        let target_id = match existing {
            Some(id) => id,
            None => s(
                &cdp.send("Target.createTarget", json!({ "url": "about:blank" }), None, CMD_TIMEOUT).await?,
                "targetId",
            ),
        };
        let mut backend = Self::open_page(cdp, events, target_id, None, options, t0).await?;
        backend.child = Some(launched.child);
        backend.profile = Some(launched.profile);
        backend.version = version;
        backend.headless_shell = launch::is_headless_shell(exe);
        backend.no_sandbox = no_sandbox;
        Ok(backend)
    }

    /// Attaches to the warm-browser daemon: takes its spare tab (renderer already
    /// booted) in a fresh browser context, or makes its own context and tab.
    pub async fn attach_daemon(r: daemon::BoxRead, w: daemon::BoxWrite, options: &SessionOptions) -> Result<Self> {
        let t0 = Instant::now();
        let (cdp, events) = Cdp::over_stream(w, r);
        let (version, spare) = tokio::join!(
            cdp.send("Browser.getVersion", json!({}), None, CMD_TIMEOUT),
            cdp.send("SwiftBrowse.claimSpare", json!({}), None, CMD_TIMEOUT),
        );
        let spare = spare?;
        let prepared = spare["sessionId"].as_str().map(str::to_string);
        let (context, target_id) = match (spare["browserContextId"].as_str(), spare["targetId"].as_str()) {
            (Some(c), Some(t)) => (c.to_string(), t.to_string()),
            _ => {
                let c = s(
                    &cdp.send("Target.createBrowserContext", json!({}), None, CMD_TIMEOUT).await?,
                    "browserContextId",
                );
                let t = cdp
                    .send(
                        "Target.createTarget",
                        json!({ "url": "about:blank", "browserContextId": c }),
                        None,
                        CMD_TIMEOUT,
                    )
                    .await?;
                (c, s(&t, "targetId"))
            }
        };
        trace(t0, "daemon tab claimed");
        let mut backend = Self::open_page(cdp, events, target_id, prepared, options, t0).await?;
        backend.version = version.ok().map(|v| s(&v, "product"));
        backend.daemon_context = Some(context);
        backend.headless_shell = true;
        Ok(backend)
    }

    /// Attaches to a page target and enables everything a session needs.
    /// `prepared`: a session the daemon already attached and set up (see
    /// [`session_setup`]); otherwise this attaches and sets up a new one.
    async fn open_page(
        cdp: Cdp,
        events: tokio::sync::mpsc::UnboundedReceiver<cdp::Event>,
        target_id: String,
        prepared: Option<String>,
        options: &SessionOptions,
        t0: Instant,
    ) -> Result<Self> {
        let trace = |phase: &str| trace(t0, phase);
        let session = match &prepared {
            Some(session) => session.clone(),
            None => {
                let attached = cdp
                    .send("Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }), None, CMD_TIMEOUT)
                    .await?;
                s(&attached, "sessionId")
            }
        };
        trace("page target attached");

        let state = Arc::new(Mutex::new(State::default()));
        let (lc_tx, lc_rx) = watch::channel(0u64);
        let pump = tokio::spawn(pump_events(events, cdp.clone(), session.clone(), state.clone(), lc_tx));

        // Independent setup commands go out together instead of one round trip each.
        let sid = Some(session.as_str());
        let setup = match prepared {
            None => session_setup(options.viewport),
            // Already done by the daemon with the default viewport.
            Some(_) if options.viewport == SessionOptions::default().viewport => Vec::new(),
            Some(_) => {
                session_setup(options.viewport).into_iter().filter(|(m, _)| m.starts_with("Emulation.")).collect()
            }
        };
        // A page target's main frame id is its target id. Set it before setup so
        // lifecycle events for the first document are recognised.
        state.lock().unwrap().main_frame = target_id.clone();
        // Sent together, confirmed together. (Not waiting for these replies before
        // the first navigation was measured: no faster, and early network events
        // could be missed.)
        let pending: Vec<cdp::Pending1> = setup.into_iter().map(|(m, p)| cdp.enqueue(m, p, sid)).collect();
        for reply in pending {
            reply
                .wait(CMD_TIMEOUT)
                .await
                .map_err(|e| CoreError::backend(format!("session setup failed: {}", e.message)))?;
        }
        trace("page domains enabled");

        Ok(Self {
            cdp,
            session,
            state,
            lifecycle: lc_rx,
            child: None,
            profile: None,
            daemon_context: None,
            pump,
            version: None,
            worlds_at_click: 0,
            headless_shell: false,
            headless: options.headless,
            no_sandbox: false,
        })
    }

    async fn send(&self, method: &str, params: Value) -> Result<Value> {
        self.cdp.send(method, params, Some(&self.session), CMD_TIMEOUT).await
    }

    async fn world(&self) -> Result<i64> {
        let (cached, frame) = {
            let st = self.state.lock().unwrap();
            (st.world, st.main_frame.clone())
        };
        if let Some(id) = cached {
            return Ok(id);
        }
        let r = self
            .send(
                "Page.createIsolatedWorld",
                json!({ "frameId": frame, "worldName": WORLD, "grantUniveralAccess": false }),
            )
            .await?;
        let id = r["executionContextId"].as_i64().ok_or_else(|| CoreError::backend("no isolated world id"))?;
        self.state.lock().unwrap().world = Some(id);
        Ok(id)
    }

    /// Calls `__sb.<func>(...args)` in the isolated world, installing the helper when needed.
    async fn call(&self, func: &str, args: Value) -> Result<Value> {
        let args = args.to_string();
        let mut last = CoreError::backend("isolated world unavailable");
        for _ in 0..4 {
            let ctx = self.world().await?;
            let installed = self.state.lock().unwrap().helper_in == Some(ctx);
            // Never assume the helper exists: Chromium can create the named world
            // in a document where the start-up script did not run.
            let call = format!("globalThis.__sb ? globalThis.__sb.{func}(...{args}) : {HELPER_MISSING:?}");
            let expr = if installed { call } else { format!("{HELPER}\n{call}") };
            let r = self
                .send(
                    "Runtime.evaluate",
                    json!({ "expression": expr, "contextId": ctx, "returnByValue": true, "awaitPromise": true }),
                )
                .await;
            match r {
                Ok(v) => {
                    if let Some(ex) = v.get("exceptionDetails") {
                        let err = js_error(ex);
                        if installed && err.message.contains("__sb") {
                            self.state.lock().unwrap().helper_in = None;
                            last = err;
                            continue;
                        }
                        return Err(err);
                    }
                    let value = v["result"].get("value").cloned().unwrap_or(Value::Null);
                    if value.as_str() == Some(HELPER_MISSING) {
                        self.state.lock().unwrap().helper_in = None;
                        last = CoreError::backend("helper missing in page world");
                        continue;
                    }
                    self.state.lock().unwrap().helper_in = Some(ctx);
                    return Ok(value);
                }
                Err(e) if context_gone(&e) => {
                    {
                        let mut st = self.state.lock().unwrap();
                        if st.world == Some(ctx) {
                            st.world = None;
                        }
                    }
                    last = e;
                    tokio::task::yield_now().await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    async fn key(&self, stroke: keys::KeyStroke) -> Result<()> {
        self.send("Input.dispatchKeyEvent", stroke.down).await?;
        self.send("Input.dispatchKeyEvent", stroke.up).await?;
        Ok(())
    }

    /// Waits for the new document's isolated world, then for its load event in the page.
    /// `Ok(None)` means the fast path could not be used and the caller should fall back.
    /// `loader` is the navigation's loader id: only the isolated world created for
    /// that document is used.
    async fn load_in_page(&mut self, loader: &str, deadline: tokio::time::Instant) -> Result<Option<Loaded>> {
        loop {
            self.lifecycle.borrow_and_update();
            {
                let st = self.state.lock().unwrap();
                if st.world.is_some() && st.world_loader == loader {
                    break;
                }
            }
            match tokio::time::timeout_at(deadline, self.lifecycle.changed()).await {
                Ok(Ok(())) => continue,
                Ok(Err(_)) => return Err(CoreError::backend("browser connection closed")),
                Err(_) => return Ok(None),
            }
        }
        let render_ms =
            RENDER_WAIT.min(deadline.saturating_duration_since(tokio::time::Instant::now())).as_millis() as u64;
        match tokio::time::timeout_at(deadline, self.call("loaded", json!([render_ms]))).await {
            Ok(Ok(v)) => {
                if std::env::var_os("SWIFTBROWSE_TRACE").is_some() {
                    let st = self.state.lock().unwrap();
                    eprintln!(
                        "[swiftbrowse] in-page load result: {v}; in flight: {:?}",
                        st.inflight.values().collect::<Vec<_>>()
                    );
                }
                Ok(serde_json::from_value(v).ok())
            }
            // Context replaced mid-load (redirect, reload) or slow page: use the lifecycle path.
            Ok(Err(_)) | Err(_) => Ok(None),
        }
    }

    async fn wait_lifecycle(&mut self, loader: &str, name: &str, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            self.lifecycle.borrow_and_update();
            if self.state.lock().unwrap().lifecycle.contains(&(loader.to_string(), name.to_string())) {
                return Ok(());
            }
            match tokio::time::timeout_at(deadline, self.lifecycle.changed()).await {
                Ok(Ok(())) => continue,
                Ok(Err(_)) => return Err(CoreError::backend("browser connection closed")),
                Err(_) => {
                    return Err(CoreError::new(
                        ErrorKind::Timeout,
                        format!("navigation did not reach {name} within {} ms", timeout.as_millis()),
                    ))
                }
            }
        }
    }
}

impl Drop for ChromiumBackend {
    /// Sessions dropped without `close` must not leak a browser or its profile:
    /// kill the process and let it exit before the profile directory is removed.
    fn drop(&mut self) {
        self.pump.abort();
        if let Some(child) = self.child.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.start_kill();
                let deadline = Instant::now() + Duration::from_secs(2);
                while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        // Helper processes can still be writing for a moment after the browser
        // exits, which makes a single delete fail; retry until the profile is gone.
        if let Some(profile) = self.profile.take() {
            let path = profile.path().to_path_buf();
            drop(profile);
            let deadline = Instant::now() + Duration::from_secs(3);
            while path.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
}

/// Commands every page session runs before use. The daemon runs them on its
/// spare tab ahead of time so a claiming client can skip them.
pub(crate) fn session_setup(viewport: (u32, u32)) -> Vec<(&'static str, Value)> {
    vec![
        ("Page.enable", json!({})),
        ("Page.setLifecycleEventsEnabled", json!({ "enabled": true })),
        ("Runtime.enable", json!({})),
        ("Network.enable", json!({})),
        ("Log.enable", json!({})),
        // Chromium creates the isolated world and runs the helper itself on every
        // new document, saving two round trips after each navigation.
        ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": HELPER, "worldName": WORLD })),
        (
            "Emulation.setDeviceMetricsOverride",
            json!({ "width": viewport.0, "height": viewport.1, "deviceScaleFactor": 1, "mobile": false }),
        ),
    ]
}

static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Event timings (ms since process start) on stderr when `SWIFTBROWSE_TRACE` is set.
fn trace_event(what: &str) {
    if std::env::var_os("SWIFTBROWSE_TRACE").is_some() {
        let t0 = *T0.get_or_init(Instant::now);
        eprintln!("[swiftbrowse] @{:.1} {what}", t0.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Phase timings on stderr when `SWIFTBROWSE_TRACE` is set.
fn trace(since: Instant, phase: &str) {
    if std::env::var_os("SWIFTBROWSE_TRACE").is_some() {
        eprintln!("[swiftbrowse] {phase}: {:.1} ms", since.elapsed().as_secs_f64() * 1000.0);
    }
}

fn context_gone(e: &CoreError) -> bool {
    ["Cannot find context", "Execution context was destroyed", "Inspected target navigated"]
        .iter()
        .any(|m| e.message.contains(m))
}

fn js_error(ex: &Value) -> CoreError {
    let desc =
        ex["exception"].get("description").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| s(ex, "text"));
    if let Some(idx) = desc.find("SB:") {
        let rest = desc[idx + 3..].lines().next().unwrap_or_default();
        let (kind, msg) = rest.split_once(':').unwrap_or(("backend", rest));
        let kind = match kind {
            "stale_element" => ErrorKind::StaleElement,
            "not_actionable" => ErrorKind::NotActionable,
            "not_found" => ErrorKind::NotFound,
            "invalid_argument" => ErrorKind::InvalidArgument,
            _ => ErrorKind::Backend,
        };
        return CoreError::new(kind, msg);
    }
    CoreError::backend(format!("page script error: {}", desc.lines().next().unwrap_or_default()))
}

async fn pump_events(
    mut events: tokio::sync::mpsc::UnboundedReceiver<Event>,
    cdp: Cdp,
    session: String,
    state: Arc<Mutex<State>>,
    lifecycle: watch::Sender<u64>,
) {
    while let Some(ev) = events.recv().await {
        if ev.session_id.as_deref() != Some(session.as_str()) {
            continue;
        }
        let p = &ev.params;
        let mut st = state.lock().unwrap();
        match ev.method.as_str() {
            "Page.lifecycleEvent" => {
                if s(p, "frameId") == st.main_frame {
                    if st.lifecycle.len() > 1_000 {
                        st.lifecycle.clear();
                    }
                    st.lifecycle.insert((s(p, "loaderId"), s(p, "name")));
                    if s(p, "name") == "init" {
                        st.last_init_loader = s(p, "loaderId");
                    }
                    trace_event(&format!("lifecycle {}", s(p, "name")));
                    lifecycle.send_modify(|n| *n += 1);
                }
            }
            "Runtime.executionContextCreated" => {
                let c = &p["context"];
                if s(c, "name") == WORLD && s(&c["auxData"], "frameId") == st.main_frame {
                    let id = c["id"].as_i64();
                    st.world = id;
                    st.helper_in = id;
                    st.worlds_created += 1;
                    st.world_loader = st.last_init_loader.clone();
                    trace_event("world created");
                    lifecycle.send_modify(|n| *n += 1);
                    if std::env::var_os("SWIFTBROWSE_TRACE").is_some() {
                        eprintln!("[swiftbrowse] new document in main frame (world #{})", st.worlds_created);
                    }
                }
            }
            "Runtime.executionContextDestroyed" => {
                if st.world == p["executionContextId"].as_i64() {
                    st.world = None;
                }
            }
            "Runtime.executionContextsCleared" => st.world = None,
            "Runtime.consoleAPICalled" => {
                let kind = s(p, "type");
                if kind == "error" || kind == "assert" {
                    let text = p["args"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|x| match x.get("value") {
                                    Some(Value::String(v)) => v.clone(),
                                    Some(v) => v.to_string(),
                                    None => x.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    let url = p["stackTrace"]["callFrames"][0].get("url").and_then(Value::as_str).map(str::to_string);
                    push_bounded(&mut st.console, ConsoleEntry { level: kind, text, url, timestamp_ms: now_ms() });
                }
            }
            "Runtime.exceptionThrown" => {
                let d = &p["exceptionDetails"];
                let text = d["exception"]
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| s(d, "text"));
                let url = d.get("url").and_then(Value::as_str).map(str::to_string);
                push_bounded(
                    &mut st.console,
                    ConsoleEntry { level: "exception".into(), text, url, timestamp_ms: now_ms() },
                );
            }
            "Log.entryAdded" => {
                let e = &p["entry"];
                if s(e, "level") == "error" && s(e, "source") != "network" {
                    let url = e.get("url").and_then(Value::as_str).map(str::to_string);
                    push_bounded(
                        &mut st.console,
                        ConsoleEntry { level: "error".into(), text: s(e, "text"), url, timestamp_ms: now_ms() },
                    );
                }
            }
            "Network.requestWillBeSent" => {
                if st.requests.len() > 5_000 {
                    st.requests.clear();
                }
                st.requests.insert(s(p, "requestId"), (s(&p["request"], "url"), s(&p["request"], "method")));
                if matches!(s(p, "type").as_str(), "Script" | "Fetch" | "XHR") {
                    if st.inflight.len() > 1_000 {
                        st.inflight.clear();
                    }
                    st.inflight.insert(s(p, "requestId"), s(&p["request"], "url"));
                }
            }
            "Network.responseReceived" => {
                let status = p["response"]["status"].as_u64().unwrap_or(0) as u16;
                if s(p, "type") == "Document" {
                    st.doc_status.insert(s(p, "loaderId"), status);
                }
                if status >= 400 {
                    let method =
                        st.requests.get(&s(p, "requestId")).map(|r| r.1.clone()).unwrap_or_else(|| "GET".into());
                    push_bounded(
                        &mut st.network,
                        NetworkFailure {
                            url: s(&p["response"], "url"),
                            method,
                            status: Some(status),
                            error: None,
                            resource_type: s(p, "type"),
                            timestamp_ms: now_ms(),
                        },
                    );
                }
            }
            "Network.loadingFinished" => {
                st.requests.remove(&s(p, "requestId"));
                st.inflight.remove(&s(p, "requestId"));
            }
            "Network.loadingFailed" => {
                st.inflight.remove(&s(p, "requestId"));
                let req = st.requests.remove(&s(p, "requestId"));
                if !p["canceled"].as_bool().unwrap_or(false) {
                    let (url, method) = req.unwrap_or_default();
                    push_bounded(
                        &mut st.network,
                        NetworkFailure {
                            url,
                            method,
                            status: None,
                            error: Some(s(p, "errorText")),
                            resource_type: s(p, "type"),
                            timestamp_ms: now_ms(),
                        },
                    );
                }
            }
            "Page.javascriptDialogOpening" => {
                // Never confirm on the user's behalf: alerts are acknowledged,
                // confirm/prompt are dismissed. Recorded for the agent to see.
                let kind = s(p, "type");
                let accept = kind == "alert" || kind == "beforeunload";
                push_bounded(
                    &mut st.dialogs,
                    DialogEvent {
                        kind,
                        message: s(p, "message"),
                        handled: if accept { "accepted" } else { "dismissed" }.into(),
                        timestamp_ms: now_ms(),
                    },
                );
                let cdp = cdp.clone();
                let session = session.clone();
                tokio::spawn(async move {
                    let _ = cdp
                        .send("Page.handleJavaScriptDialog", json!({ "accept": accept }), Some(&session), CMD_TIMEOUT)
                        .await;
                });
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Backend for ChromiumBackend {
    fn capabilities(&self) -> CapabilityReport {
        let mut warnings = Vec::new();
        if self.daemon_context.is_some() {
            warnings.push(
                "shared warm browser (daemon): this session is isolated by its own browser context, not by a separate process"
                    .into(),
            );
        }
        if self.no_sandbox {
            warnings.push(
                "Chromium sandbox disabled (running as root or SWIFTBROWSE_NO_SANDBOX=1); use a container boundary"
                    .into(),
            );
        }
        CapabilityReport {
            backend: "chromium".into(),
            engine: if self.headless_shell { "chromium-headless-shell" } else { "chromium" }.into(),
            engine_version: self.version.clone(),
            headless: self.headless,
            capabilities: vec![
                cap(names::NAVIGATE, Support::Full, None),
                cap(names::SNAPSHOT, Support::Full, None),
                cap(names::ACCESSIBILITY_TREE, Support::Full, None),
                cap(names::SEMANTIC_LOCATORS, Support::Full, Some("role/name computed by SwiftBrowse; close to, not identical with, the browser's accessibility tree")),
                cap(names::CSS_XPATH, Support::Full, None),
                cap(names::TRUSTED_INPUT, Support::Full, None),
                cap(names::SELECT_OPTION, Support::Full, None),
                cap(names::SCREENSHOT, Support::Full, None),
                cap(names::CONSOLE, Support::Full, None),
                cap(names::NETWORK, Support::Full, None),
                cap(names::DIALOGS, Support::Partial, Some("alerts accepted, confirm/prompt dismissed automatically and reported")),
                cap(names::FRAMES, Support::Partial, Some("main frame only")),
                cap(names::SHADOW_DOM, Support::Partial, Some("open shadow roots for semantic and text locators; CSS does not pierce")),
                if self.daemon_context.is_some() {
                    cap(names::ISOLATED_SESSIONS, Support::Partial, Some("separate browser context per session in a shared browser"))
                } else {
                    cap(names::ISOLATED_SESSIONS, Support::Full, Some("separate process and profile per session"))
                },
                cap(names::DOWNLOADS, Support::Unsupported, None),
                cap(names::UPLOADS, Support::Unsupported, None),
                cap(names::VISUAL_GROUNDING, Support::Unsupported, None),
            ],
            warnings,
        }
    }

    async fn navigate(&mut self, url: &str, wait_until: WaitUntil, timeout: Duration) -> Result<NavigationInfo> {
        let started = Instant::now();
        trace_event("navigate start");
        let r = self.cdp.send("Page.navigate", json!({ "url": url }), Some(&self.session), timeout).await?;
        trace_event("Page.navigate replied");
        let error = s(&r, "errorText");
        if !error.is_empty() {
            return Err(CoreError::new(ErrorKind::NavigationFailed, format!("{url}: {error}")));
        }
        let loader = s(&r, "loaderId");
        let same_document = loader.is_empty();
        // Fast path for `load`: one evaluation in the new document that waits for
        // the load event inside the page and returns URL and title, instead of
        // waiting for the event here and then asking the page in a second round trip.
        if !same_document && wait_until == WaitUntil::Load {
            let deadline = tokio::time::Instant::from_std(started + timeout);
            if let Some(page) = self.load_in_page(&loader, deadline).await? {
                trace_event("in-page load resolved");
                let status = self.state.lock().unwrap().doc_status.get(&loader).copied();
                return Ok(NavigationInfo {
                    url: page.url,
                    title: page.title,
                    status,
                    same_document,
                    rendered_after_load_ms: page.rendered_after_load_ms,
                    still_empty: page.still_empty,
                    still_changing: page.still_changing,
                });
            }
        }
        if !same_document {
            let name = match wait_until {
                WaitUntil::DomContentLoaded => "DOMContentLoaded",
                WaitUntil::Load => "load",
                WaitUntil::NetworkIdle => "networkIdle",
            };
            self.wait_lifecycle(&loader, name, timeout.saturating_sub(started.elapsed())).await?;
            trace(started, name);
        }
        let status = self.state.lock().unwrap().doc_status.get(&loader).copied();
        let mut settled = Loaded::default();
        if !same_document && wait_until != WaitUntil::NetworkIdle {
            let render_ms = RENDER_WAIT.min(timeout.saturating_sub(started.elapsed())).as_millis() as u64;
            if let Ok(v) = self.call("settle", json!([render_ms])).await {
                settled = serde_json::from_value(v).unwrap_or_default();
            }
        }
        // Read from the page: Chromium's pushed target title still holds the URL
        // placeholder when `load` fires, so it cannot be trusted here.
        let page = self.page_state().await?;
        trace(started, "page state read");
        Ok(NavigationInfo {
            url: page.url,
            title: page.title,
            status,
            same_document,
            rendered_after_load_ms: settled.rendered_after_load_ms,
            still_empty: settled.still_empty,
            still_changing: settled.still_changing,
        })
    }

    async fn page_state(&mut self) -> Result<PageState> {
        let v = self.call("pageState", json!([])).await?;
        serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()))
    }

    async fn snapshot(&mut self, mode: SnapshotMode, max_elements: usize) -> Result<PageSnapshot> {
        let mode = if mode == SnapshotMode::Full { "full" } else { "compact" };
        let v = self.call("snapshot", json!([mode, max_elements, RENDER_WAIT.as_millis() as u64])).await?;
        let mut snap: PageSnapshot = serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()))?;
        let mut st = self.state.lock().unwrap();
        let from = st.dialogs_reported.min(st.dialogs.len());
        for d in st.dialogs.iter().skip(from) {
            snap.messages.push(Message {
                kind: "js_dialog".into(),
                text: format!("{} \"{}\" was {}", d.kind, d.message, d.handled),
                ref_id: None,
            });
        }
        st.dialogs_reported = st.dialogs.len();
        Ok(snap)
    }

    async fn query(&mut self, locator: &Locator) -> Result<Vec<ElementInfo>> {
        if matches!(locator, Locator::Ref { .. } | Locator::Auto { .. }) {
            return Err(CoreError::invalid("ref and auto locators are resolved by the core"));
        }
        let v = self.call("query", json!([locator])).await?;
        serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()))
    }

    async fn describe(&mut self, ref_id: &str) -> Result<ElementInfo> {
        let v = self.call("describe", json!([ref_id])).await?;
        serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()))
    }

    async fn read_value(&mut self, ref_id: &str) -> Result<Option<String>> {
        let v = self.call("readValue", json!([ref_id])).await?;
        Ok(v.as_str().map(str::to_string))
    }

    async fn click(&mut self, ref_id: &str) -> Result<()> {
        let pt = self.call("prepareClick", json!([ref_id])).await?;
        self.worlds_at_click = self.state.lock().unwrap().worlds_created;
        let (x, y) = (pt["x"].as_f64().unwrap_or(0.0), pt["y"].as_f64().unwrap_or(0.0));
        // Queue move, press and release together: waiting for each acknowledgement
        // leaves time for the page to shift the layout between press and release.
        let events = [
            json!({ "type": "mouseMoved", "x": x, "y": y, "buttons": 0 }),
            json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": 1 }),
            json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "buttons": 0, "clickCount": 1 }),
        ];
        let pending: Vec<_> =
            events.into_iter().map(|e| self.cdp.enqueue("Input.dispatchMouseEvent", e, Some(&self.session))).collect();
        for p in pending {
            // Input may have reached the page: never report a retryable kind from here.
            p.wait(CMD_TIMEOUT)
                .await
                .map_err(|e| CoreError::backend(format!("click dispatch failed: {}", e.message)))?;
        }
        // The click guard blocked a click that would have missed, before the page
        // acted on it, so it is safe to retry. A navigation away from the page
        // (the call fails) means the click landed.
        if let Ok(done) = self.call("clickDone", json!([])).await {
            if done["missed"].as_bool() == Some(true) {
                let hit = done["hit"].as_str().unwrap_or("another element");
                let when = if done["seen"].as_u64().unwrap_or(0) > 0 { "during" } else { "before" };
                return Err(CoreError::new(
                    ErrorKind::NotActionable,
                    format!("{ref_id} moved {when} the click, which would have hit {hit}; the click was blocked"),
                ));
            }
            // No mouse event arrived at all: a just-loaded page can drop input
            // until it is ready.
            if done["seen"].as_u64() == Some(0) {
                return Err(CoreError::new(
                    ErrorKind::NotActionable,
                    "the page did not receive the click (it was not ready for input yet)",
                ));
            }
        }
        Ok(())
    }

    async fn fill(&mut self, ref_id: &str, value: &str) -> Result<()> {
        let done = self.call("prepareFill", json!([ref_id, value])).await?;
        if done.as_bool() == Some(true) {
            return Ok(());
        }
        let r = if value.is_empty() {
            self.key(keys::parse("Delete")?).await
        } else {
            self.send("Input.insertText", json!({ "text": value })).await.map(|_| ())
        };
        r.map_err(|e| CoreError::backend(format!("text input failed: {}", e.message)))
    }

    async fn focus(&mut self, ref_id: &str) -> Result<()> {
        self.call("focus", json!([ref_id])).await.map(|_| ())
    }

    async fn type_text(&mut self, text: &str) -> Result<()> {
        for c in text.chars() {
            let stroke = if c == '\n' {
                keys::parse("Enter")?
            } else {
                let t = c.to_string();
                keys::KeyStroke {
                    down: json!({ "type": "keyDown", "key": t, "text": t, "unmodifiedText": t }),
                    up: json!({ "type": "keyUp", "key": t }),
                }
            };
            self.key(stroke).await?;
        }
        Ok(())
    }

    async fn press_key(&mut self, key: &str) -> Result<()> {
        self.key(keys::parse(key)?).await
    }

    async fn select_option(&mut self, ref_id: &str, option: &str) -> Result<Vec<String>> {
        let v = self.call("selectOption", json!([ref_id, option])).await?;
        serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()))
    }

    async fn hover(&mut self, ref_id: &str) -> Result<()> {
        let pt = self.call("pointAt", json!([ref_id])).await?;
        let (x, y) = (pt["x"].as_f64().unwrap_or(0.0), pt["y"].as_f64().unwrap_or(0.0));
        self.send("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y, "buttons": 0 }))
            .await
            .map(|_| ())
            .map_err(|e| CoreError::backend(format!("hover dispatch failed: {}", e.message)))
    }

    async fn read_text(&mut self, ref_id: &str) -> Result<String> {
        let v = self.call("readText", json!([ref_id])).await?;
        Ok(v.as_str().unwrap_or_default().to_string())
    }

    async fn history(&mut self, delta: i32, timeout: Duration) -> Result<NavigationInfo> {
        #[derive(serde::Deserialize, Default)]
        struct Doc {
            origin: f64,
            url: String,
            ready: String,
        }
        let started = Instant::now();
        let doc = |v: Value| serde_json::from_value::<Doc>(v).unwrap_or_default();
        let before = doc(self.call("docState", json!([])).await?);
        let target_url = if delta == 0 {
            self.send("Page.reload", json!({})).await?;
            None
        } else {
            let h = self.send("Page.getNavigationHistory", json!({})).await?;
            let entries = h["entries"].as_array().cloned().unwrap_or_default();
            let index = h["currentIndex"].as_i64().unwrap_or(0) + i64::from(delta);
            let Some(entry) = usize::try_from(index).ok().and_then(|i| entries.get(i)) else {
                let which = if delta < 0 { "back" } else { "forward" };
                return Err(CoreError::new(ErrorKind::NavigationFailed, format!("no history entry to go {which} to")));
            };
            self.send("Page.navigateToHistoryEntry", json!({ "entryId": entry["id"] })).await?;
            Some(s(entry, "url"))
        };
        // Done when a new document has loaded, or (same-document history) the URL moved.
        loop {
            if let Ok(v) = self.call("docState", json!([])).await {
                let now = doc(v);
                let new_doc = now.origin != before.origin;
                let arrived = match &target_url {
                    None => new_doc,
                    Some(u) => new_doc || (now.url == *u && now.url != before.url),
                };
                if arrived && now.ready == "complete" {
                    break;
                }
            }
            if started.elapsed() >= timeout {
                return Err(CoreError::new(
                    ErrorKind::Timeout,
                    format!("page did not load within {} ms", timeout.as_millis()),
                ));
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        let render_ms = RENDER_WAIT.min(timeout.saturating_sub(started.elapsed())).as_millis() as u64;
        let settled: Loaded = self
            .call("settle", json!([render_ms]))
            .await
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let page = self.page_state().await?;
        Ok(NavigationInfo {
            url: page.url,
            title: page.title,
            status: None,
            same_document: false,
            rendered_after_load_ms: settled.rendered_after_load_ms,
            still_empty: settled.still_empty,
            still_changing: settled.still_changing,
        })
    }

    async fn evaluate(&mut self, expression: &str, timeout: Duration) -> Result<Value> {
        let r = self
            .cdp
            .send(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": true, "awaitPromise": true, "userGesture": true }),
                Some(&self.session),
                timeout,
            )
            .await?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(js_error(ex));
        }
        Ok(r["result"].get("value").cloned().unwrap_or(Value::Null))
    }

    async fn accessibility_tree(&mut self, max_nodes: usize) -> Result<Value> {
        let r = self.send("Accessibility.getFullAXTree", json!({})).await?;
        let nodes = r["nodes"].as_array().cloned().unwrap_or_default();
        let by_id: HashMap<String, &Value> = nodes.iter().map(|n| (s(n, "nodeId"), n)).collect();
        let skip = |n: &Value| {
            let role = s(&n["role"], "value");
            n["ignored"].as_bool().unwrap_or(false)
                || matches!(role.as_str(), "InlineTextBox" | "none" | "LineBreak")
                || (matches!(role.as_str(), "generic" | "StaticText") && s(&n["name"], "value").trim().is_empty())
        };
        let mut out = Vec::new();
        let mut total = 0;
        for n in &nodes {
            if skip(n) {
                continue;
            }
            total += 1;
            if out.len() >= max_nodes {
                continue;
            }
            let mut parent = n.get("parentId").and_then(Value::as_str).map(str::to_string);
            while let Some(p) = parent.as_ref().and_then(|p| by_id.get(p)) {
                if !skip(p) {
                    break;
                }
                parent = p.get("parentId").and_then(Value::as_str).map(str::to_string);
            }
            let mut node = json!({ "id": s(n, "nodeId"), "role": s(&n["role"], "value"), "name": s(&n["name"], "value"), "parent": parent });
            if let Some(v) = n["value"].get("value") {
                node["value"] = v.clone();
            }
            out.push(node);
        }
        Ok(json!({ "nodes": out, "total": total, "truncated": total > out.len() }))
    }

    async fn screenshot(&mut self, path: &Path, full_page: bool) -> Result<()> {
        let mut params = json!({ "format": "png" });
        if full_page {
            let m = self.send("Page.getLayoutMetrics", json!({})).await?;
            let size = &m["cssContentSize"];
            params["captureBeyondViewport"] = json!(true);
            params["clip"] = json!({ "x": 0, "y": 0, "width": size["width"], "height": size["height"], "scale": 1 });
        }
        let r = self.send("Page.captureScreenshot", params).await?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(s(&r, "data"))
            .map_err(|e| CoreError::backend(format!("bad screenshot data: {e}")))?;
        tokio::fs::write(path, bytes)
            .await
            .map_err(|e| CoreError::backend(format!("cannot write {}: {e}", path.display())))
    }

    async fn click_effect(&mut self, max: Duration) -> Result<Option<Vec<String>>> {
        let new_document = |me: &Self| me.state.lock().unwrap().worlds_created > me.worlds_at_click;
        // A document replaced since the click is the strongest possible effect; the
        // new document's helper knows nothing about the click, so ask the state first.
        if new_document(self) {
            return Ok(Some(vec!["navigated".into()]));
        }
        match self.call("clickEffect", json!([max.as_millis() as u64])).await {
            Ok(_) if new_document(self) => Ok(Some(vec!["navigated".into()])),
            Ok(v) => Ok(serde_json::from_value(v).ok().flatten()),
            Err(e) if context_gone(&e) || new_document(self) => Ok(Some(vec!["navigated".into()])),
            Err(e) => Err(e),
        }
    }

    async fn console_errors(&mut self) -> Result<Vec<ConsoleEntry>> {
        Ok(self.state.lock().unwrap().console.iter().cloned().collect())
    }

    async fn network_failures(&mut self) -> Result<Vec<NetworkFailure>> {
        Ok(self.state.lock().unwrap().network.iter().cloned().collect())
    }

    async fn dialogs(&mut self) -> Result<Vec<DialogEvent>> {
        Ok(self.state.lock().unwrap().dialogs.iter().cloned().collect())
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(context) = self.daemon_context.take() {
            // Shared browser: drop only this session's context (and its data).
            let params = json!({ "browserContextId": context });
            let _ = self.cdp.send("Target.disposeBrowserContext", params, None, Duration::from_secs(3)).await;
        } else if let Some(child) = self.child.as_mut() {
            let _ = self.cdp.send("Browser.close", json!({}), None, Duration::from_secs(3)).await;
            if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
        }
        self.pump.abort();
        Ok(())
    }
}
