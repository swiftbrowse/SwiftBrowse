//! Chromium backend: one browser process and one isolated browser context per
//! session, driven over the Chrome DevTools Protocol.

mod cdp;
pub mod daemon;
mod keys;
pub mod launch;
mod video;

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
    Backend, BackendFactory, CapabilityReport, CoreError, ErrorKind, FrameRef, Locator, Result, SessionOptions, Support,
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
            // Concurrent sessions of this process each get their own warm daemon
            // (browser); sequential ones keep reusing slot 0.
            let slot = SlotGuard::acquire();
            let mut args = args.clone();
            if slot.0 > 0 {
                args.extend(["--slot".to_string(), slot.0.to_string()]);
            }
            let spawn = (program.clone(), args);
            let connected = daemon::with_slot(slot.0, daemon::connect_or_spawn(Some(&spawn))).await;
            if let Some((r, w)) = connected {
                match ChromiumBackend::attach_daemon(r, w, options).await {
                    Ok(mut backend) => {
                        backend.slot = Some(slot);
                        return Ok(Box::new(backend));
                    }
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

/// A daemon slot held by one session, machine-wide (see [`daemon::lease_slot`]);
/// released with the lease file when dropped.
struct SlotGuard(usize, #[allow(dead_code)] Option<std::fs::File>);

impl SlotGuard {
    /// The lowest free slot; when every slot is busy, sessions share the last one.
    fn acquire() -> Self {
        match daemon::lease_slot() {
            Some((slot, lease)) => SlotGuard(slot, Some(lease)),
            None => SlotGuard(daemon::max_slots() - 1, None),
        }
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
    /// Main-world execution context of each same-process frame (for `evaluate` in a frame).
    frame_contexts: HashMap<String, i64>,
}

/// One page target (tab) of this session.
struct Tab {
    target_id: String,
    session: String,
    state: Arc<Mutex<State>>,
    lifecycle: watch::Receiver<u64>,
    pump: tokio::task::JoinHandle<()>,
}

/// A page target this session can see, from `Target.*` events.
#[derive(Clone, Debug, Default)]
struct TargetSeen {
    target_id: String,
    url: String,
    title: String,
    opener: Option<String>,
}

/// Shared between the event router and the backend.
#[derive(Default)]
struct Shared {
    /// Event channel of each attached page session.
    routes: Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<Event>>>,
    /// Page targets in creation order.
    pages: Mutex<Vec<TargetSeen>>,
    /// Out-of-process iframes: frame (= target) id -> session id.
    iframes: Mutex<HashMap<String, String>>,
    /// Network routes (mocks), oldest first; the newest match wins.
    mocks: Mutex<Vec<Mock>>,
    next_mock: std::sync::atomic::AtomicU64,
    /// Requests made by every tab of the session, oldest first.
    requests: Mutex<VecDeque<RequestRecord>>,
    /// Network request ids answered by a route.
    mocked: Mutex<HashSet<String>>,
    /// Screencast frames while a video is being recorded.
    screencast: Mutex<Option<VecDeque<video::Frame>>>,
    /// The session being recorded (the active tab), for filler captures.
    video_session: Mutex<String>,
    /// When the last frame arrived, from the screencast or a filler capture.
    last_frame: Mutex<Option<Instant>>,
    /// Frames the browser streamed, and frames captured to fill gaps.
    streamed: std::sync::atomic::AtomicUsize,
    filled: std::sync::atomic::AtomicUsize,
}

struct Mock {
    id: String,
    spec: swiftbrowse_core::request::RouteArgs,
    url: regex::Regex,
    hits: u32,
}

const MAX_REQUESTS: usize = 1_000;

/// One level of the frame the session is working in.
#[derive(Clone, Debug)]
struct FrameLevel {
    frame_id: String,
    /// Session hosting this frame's document (the page, or an out-of-process iframe).
    session: String,
    /// The `<iframe>` element in the parent document, and the session hosting it.
    owner: i64,
    owner_session: String,
}

pub struct ChromiumBackend {
    cdp: Cdp,
    /// The tab actions go to.
    tab: Tab,
    /// Other open tabs.
    others: Vec<Tab>,
    shared: Arc<Shared>,
    router: tokio::task::JoinHandle<()>,
    viewport: (u32, u32),
    /// Frames entered with `enter_frame`, outermost first; empty means the main frame.
    frame: Vec<FrameLevel>,
    /// Isolated world (context id, helper installed) in the innermost entered frame.
    frame_world: Mutex<Option<(i64, bool)>>,
    /// The browser process, when this session owns one (not when attached to the daemon).
    child: Option<tokio::process::Child>,
    profile: Option<tempfile::TempDir>,
    /// The browser context this session owns inside the shared daemon browser.
    daemon_context: Option<String>,
    /// The daemon slot this session holds (freed with the backend).
    slot: Option<SlotGuard>,
    version: Option<String>,
    headless_shell: bool,
    /// Document generation when the last click was dispatched.
    worlds_at_click: u64,
    /// Number of tabs when the last click was dispatched.
    tabs_at_click: usize,
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
    /// Launches a browser for this session. If it aborts for lack of a usable
    /// sandbox, it is started once more without one (and the process remembers).
    pub async fn start(exe: &Path, options: &SessionOptions, no_sandbox: bool, extra: &[String]) -> Result<Self> {
        match Self::start_once(exe, options, no_sandbox, extra).await {
            Err(e) if !no_sandbox && e.message.contains("No usable sandbox") => {
                launch::SANDBOX_UNUSABLE.store(true, std::sync::atomic::Ordering::Relaxed);
                Self::start_once(exe, options, true, extra).await
            }
            other => other,
        }
    }

    async fn start_once(exe: &Path, options: &SessionOptions, no_sandbox: bool, extra: &[String]) -> Result<Self> {
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
        // Discovery reports tabs the page opens (popups, target=_blank). (The
        // daemon does this itself and forwards a client its own tabs.)
        let (version, targets, _) = tokio::join!(
            cdp.send("Browser.getVersion", json!({}), None, CMD_TIMEOUT),
            cdp.send("Target.getTargets", json!({}), None, CMD_TIMEOUT),
            cdp.send("Target.setDiscoverTargets", json!({ "discover": true }), None, CMD_TIMEOUT),
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

    /// Starts the event router and attaches the first tab.
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
        let shared = Arc::new(Shared::default());
        shared.pages.lock().unwrap().push(TargetSeen { target_id: target_id.clone(), ..Default::default() });
        let router = tokio::spawn(route_events(events, shared.clone()));
        let tab = open_tab(&cdp, &shared, target_id, prepared, options.viewport, t0).await?;
        Ok(Self {
            cdp,
            tab,
            others: Vec::new(),
            shared,
            router,
            viewport: options.viewport,
            frame: Vec::new(),
            frame_world: Mutex::new(None),
            child: None,
            profile: None,
            daemon_context: None,
            slot: None,
            version: None,
            worlds_at_click: 0,
            tabs_at_click: 0,
            headless_shell: false,
            headless: options.headless,
            no_sandbox: false,
        })
    }

    async fn send(&self, method: &str, params: Value) -> Result<Value> {
        self.cdp.send(method, params, Some(&self.tab.session), CMD_TIMEOUT).await
    }

    async fn world(&self) -> Result<i64> {
        let (cached, frame) = {
            let st = self.tab.state.lock().unwrap();
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
        self.tab.state.lock().unwrap().world = Some(id);
        Ok(id)
    }

    /// Calls `__sb.<func>(...args)` in the isolated world, installing the helper when needed.
    async fn call(&self, func: &str, args: Value) -> Result<Value> {
        if !self.frame.is_empty() {
            return self.frame_call(func, args).await;
        }
        let args = args.to_string();
        let mut last = CoreError::backend("isolated world unavailable");
        for _ in 0..4 {
            let ctx = self.world().await?;
            let installed = self.tab.state.lock().unwrap().helper_in == Some(ctx);
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
                            self.tab.state.lock().unwrap().helper_in = None;
                            last = err;
                            continue;
                        }
                        return Err(err);
                    }
                    let value = v["result"].get("value").cloned().unwrap_or(Value::Null);
                    if value.as_str() == Some(HELPER_MISSING) {
                        self.tab.state.lock().unwrap().helper_in = None;
                        last = CoreError::backend("helper missing in page world");
                        continue;
                    }
                    self.tab.state.lock().unwrap().helper_in = Some(ctx);
                    return Ok(value);
                }
                Err(e) if context_gone(&e) => {
                    {
                        let mut st = self.tab.state.lock().unwrap();
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

    /// `call` inside the entered frame: its own isolated world, made on demand.
    async fn frame_call(&self, func: &str, args: Value) -> Result<Value> {
        let level = self.frame.last().expect("in a frame");
        let args = args.to_string();
        let mut last = CoreError::backend("frame world unavailable");
        for _ in 0..4 {
            let cached = *self.frame_world.lock().unwrap();
            let (ctx, installed) = match cached {
                Some(w) => w,
                None => {
                    let params = json!({ "frameId": level.frame_id, "worldName": WORLD, "grantUniveralAccess": false });
                    let r = self
                        .cdp
                        .send("Page.createIsolatedWorld", params, Some(&level.session), CMD_TIMEOUT)
                        .await
                        .map_err(|e| {
                            CoreError::new(
                                ErrorKind::StaleElement,
                                format!("the frame is gone or not loaded: {}", e.message),
                            )
                        })?;
                    let id = r["executionContextId"].as_i64().ok_or_else(|| CoreError::backend("no frame world id"))?;
                    (id, false)
                }
            };
            let call = format!("globalThis.__sb ? globalThis.__sb.{func}(...{args}) : {HELPER_MISSING:?}");
            let expr = if installed { call } else { format!("{HELPER}\n{call}") };
            let params = json!({ "expression": expr, "contextId": ctx, "returnByValue": true, "awaitPromise": true });
            match self.cdp.send("Runtime.evaluate", params, Some(&level.session), CMD_TIMEOUT).await {
                Ok(v) => {
                    if let Some(ex) = v.get("exceptionDetails") {
                        let err = js_error(ex);
                        if installed && err.message.contains("__sb") {
                            *self.frame_world.lock().unwrap() = Some((ctx, false));
                            last = err;
                            continue;
                        }
                        return Err(err);
                    }
                    let value = v["result"].get("value").cloned().unwrap_or(Value::Null);
                    if value.as_str() == Some(HELPER_MISSING) {
                        *self.frame_world.lock().unwrap() = Some((ctx, false));
                        continue;
                    }
                    *self.frame_world.lock().unwrap() = Some((ctx, true));
                    return Ok(value);
                }
                Err(e) if context_gone(&e) => {
                    *self.frame_world.lock().unwrap() = None;
                    last = e;
                    tokio::task::yield_now().await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    /// Session hosting the document actions currently go to.
    fn context_session(&self) -> &str {
        self.frame.last().map(|f| f.session.as_str()).unwrap_or(&self.tab.session)
    }

    /// Where the entered frame's viewport sits in the page's viewport, scrolling
    /// the `<iframe>` into view first.
    async fn frame_offset(&self) -> Result<(f64, f64)> {
        let (mut x, mut y) = (0.0, 0.0);
        for (i, level) in self.frame.iter().enumerate() {
            // Box models are relative to the hosting session's top frame, so within
            // one session only the innermost `<iframe>` counts.
            if self.frame.get(i + 1).is_some_and(|next| next.owner_session == level.owner_session) {
                continue;
            }
            let node = json!({ "backendNodeId": level.owner });
            let _ = self
                .cdp
                .send("DOM.scrollIntoViewIfNeeded", node.clone(), Some(&level.owner_session), CMD_TIMEOUT)
                .await;
            let m = self.cdp.send("DOM.getBoxModel", node, Some(&level.owner_session), CMD_TIMEOUT).await?;
            let q = &m["model"]["content"];
            x += q[0].as_f64().unwrap_or(0.0);
            y += q[1].as_f64().unwrap_or(0.0);
        }
        Ok((x, y))
    }

    /// The element behind a helper ref, as a remote object id in the current frame.
    async fn element_object(&self, ref_id: &str) -> Result<String> {
        // Validates the ref (stale, unknown) with the helper's own errors first.
        self.call("describe", json!([ref_id])).await?;
        let (session, ctx) = match self.frame.last() {
            Some(level) => (level.session.clone(), self.frame_world.lock().unwrap().map(|w| w.0)),
            None => (self.tab.session.clone(), self.tab.state.lock().unwrap().world),
        };
        let ctx =
            ctx.ok_or_else(|| CoreError::new(ErrorKind::StaleElement, "the page changed; look the element up again"))?;
        let params = json!({ "expression": format!("globalThis.__sb.element({ref_id:?})"), "contextId": ctx });
        let r = self.cdp.send("Runtime.evaluate", params, Some(&session), CMD_TIMEOUT).await?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(js_error(ex));
        }
        r["result"]["objectId"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| CoreError::backend("element has no object id"))
    }

    /// URL and title of the active tab's main document, even inside a frame.
    async fn page_state_of_tab(&self) -> Result<PageState> {
        if self.frame.is_empty() {
            let v = self.call("pageState", json!([])).await?;
            return serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()));
        }
        let r = self
            .send(
                "Runtime.evaluate",
                json!({ "expression": "({ url: location.href, title: document.title })", "returnByValue": true }),
            )
            .await?;
        serde_json::from_value(r["result"]["value"].clone()).map_err(|e| CoreError::backend(e.to_string()))
    }

    /// Navigates and waits for DOMContentLoaded only: for internal pages, without
    /// the render settling that `navigate` does for real ones.
    async fn navigate_bare(&mut self, url: &str) -> Result<()> {
        self.leave_frames();
        let r = self.send("Page.navigate", json!({ "url": url })).await?;
        let error = s(&r, "errorText");
        if !error.is_empty() {
            return Err(CoreError::new(ErrorKind::NavigationFailed, format!("{url}: {error}")));
        }
        let loader = s(&r, "loaderId");
        if !loader.is_empty() {
            self.wait_lifecycle(&loader, "DOMContentLoaded", Duration::from_secs(10)).await?;
        }
        Ok(())
    }

    fn screencast_params(&self) -> Value {
        json!({ "format": "jpeg", "quality": 70, "maxWidth": self.viewport.0, "maxHeight": self.viewport.1, "everyNthFrame": 1 })
    }

    fn tab_sessions(&self) -> Vec<String> {
        std::iter::once(&self.tab).chain(&self.others).map(|t| t.session.clone()).collect()
    }

    fn leave_frames(&mut self) {
        self.frame.clear();
        *self.frame_world.lock().unwrap() = None;
    }

    /// Makes the tab with this target active, attaching to it first if needed.
    async fn activate(&mut self, target_id: &str) -> Result<()> {
        if self.tab.target_id == target_id {
            return Ok(());
        }
        let tab = match self.others.iter().position(|t| t.target_id == target_id) {
            Some(i) => self.others.remove(i),
            None => {
                open_tab(&self.cdp, &self.shared, target_id.to_string(), None, self.viewport, Instant::now()).await?
            }
        };
        let previous = std::mem::replace(&mut self.tab, tab);
        // A recording follows the active tab.
        if self.shared.screencast.lock().unwrap().is_some() {
            *self.shared.video_session.lock().unwrap() = self.tab.session.clone();
            let _ = self.cdp.send("Page.stopScreencast", json!({}), Some(&previous.session), CMD_TIMEOUT).await;
            let _ = self.send("Page.startScreencast", self.screencast_params()).await;
        }
        self.others.push(previous);
        self.leave_frames();
        let _ = self.send("Page.bringToFront", json!({})).await;
        Ok(())
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
            self.tab.lifecycle.borrow_and_update();
            {
                let st = self.tab.state.lock().unwrap();
                if st.world.is_some() && st.world_loader == loader {
                    break;
                }
            }
            match tokio::time::timeout_at(deadline, self.tab.lifecycle.changed()).await {
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
                    let st = self.tab.state.lock().unwrap();
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
            self.tab.lifecycle.borrow_and_update();
            if self.tab.state.lock().unwrap().lifecycle.contains(&(loader.to_string(), name.to_string())) {
                return Ok(());
            }
            match tokio::time::timeout_at(deadline, self.tab.lifecycle.changed()).await {
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
        self.router.abort();
        self.tab.pump.abort();
        for t in &self.others {
            t.pump.abort();
        }
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
        // Out-of-process (cross-site) iframes get their own sessions, for `enter_frame`.
        (
            "Target.setAutoAttach",
            json!({ "autoAttach": true, "waitForDebuggerOnStart": false, "flatten": true, "filter": [{ "type": "iframe" }] }),
        ),
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

/// Attaches to a page target (unless `prepared` already is) and enables
/// everything a session needs.
async fn open_tab(
    cdp: &Cdp,
    shared: &Arc<Shared>,
    target_id: String,
    prepared: Option<String>,
    viewport: (u32, u32),
    t0: Instant,
) -> Result<Tab> {
    let session = match &prepared {
        Some(session) => session.clone(),
        None => {
            let attached = cdp
                .send("Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }), None, CMD_TIMEOUT)
                .await?;
            s(&attached, "sessionId")
        }
    };
    trace(t0, "page target attached");

    let state = Arc::new(Mutex::new(State::default()));
    // A page target's main frame id is its target id. Set it before setup so
    // lifecycle events for the first document are recognised.
    state.lock().unwrap().main_frame = target_id.clone();
    let (lc_tx, lc_rx) = watch::channel(0u64);
    let (ev_tx, ev_rx) = tokio::sync::mpsc::unbounded_channel();
    shared.routes.lock().unwrap().insert(session.clone(), ev_tx);
    let pump = tokio::spawn(pump_events(ev_rx, cdp.clone(), session.clone(), state.clone(), lc_tx, shared.clone()));

    let sid = Some(session.as_str());
    let mut setup = match prepared {
        None => session_setup(viewport),
        // Already done by the daemon with the default viewport.
        Some(_) if viewport == SessionOptions::default().viewport => Vec::new(),
        Some(_) => session_setup(viewport).into_iter().filter(|(m, _)| m.starts_with("Emulation.")).collect(),
    };
    if !shared.mocks.lock().unwrap().is_empty() {
        setup.push(("Fetch.enable", fetch_patterns()));
    }
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
    trace(t0, "page domains enabled");
    Ok(Tab { target_id, session, state, lifecycle: lc_rx, pump })
}

/// Sends each event to the tab whose session it belongs to, and keeps the
/// list of page targets and out-of-process iframes up to date.
async fn route_events(mut events: tokio::sync::mpsc::UnboundedReceiver<Event>, shared: Arc<Shared>) {
    while let Some(ev) = events.recv().await {
        let p = &ev.params;
        match ev.session_id.as_deref() {
            Some(session) => {
                match ev.method.as_str() {
                    "Target.attachedToTarget" if s(&p["targetInfo"], "type") == "iframe" => {
                        let frame = s(&p["targetInfo"], "targetId");
                        shared.iframes.lock().unwrap().insert(frame, s(p, "sessionId"));
                    }
                    "Target.detachedFromTarget" => {
                        let gone = s(p, "sessionId");
                        shared.iframes.lock().unwrap().retain(|_, s| *s != gone);
                    }
                    _ => {}
                }
                let tx = shared.routes.lock().unwrap().get(session).cloned();
                if let Some(tx) = tx {
                    let _ = tx.send(ev);
                }
            }
            None => {
                let info = &p["targetInfo"];
                match ev.method.as_str() {
                    "Target.targetCreated" | "Target.targetInfoChanged" if s(info, "type") == "page" => {
                        let seen = TargetSeen {
                            target_id: s(info, "targetId"),
                            url: s(info, "url"),
                            title: s(info, "title"),
                            opener: info.get("openerId").and_then(Value::as_str).map(str::to_string),
                        };
                        let mut pages = shared.pages.lock().unwrap();
                        match pages.iter_mut().find(|t| t.target_id == seen.target_id) {
                            Some(t) => {
                                t.url = seen.url;
                                t.title = seen.title;
                                if seen.opener.is_some() {
                                    t.opener = seen.opener;
                                }
                            }
                            None if ev.method == "Target.targetCreated" => pages.push(seen),
                            None => {}
                        }
                    }
                    "Target.targetDestroyed" => {
                        let id = s(p, "targetId");
                        shared.pages.lock().unwrap().retain(|t| t.target_id != id);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn fetch_patterns() -> Value {
    json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] })
}

/// Answers an intercepted request from the newest matching route, or lets it through.
fn answer_paused(p: &Value, cdp: &Cdp, session: &str, shared: &Shared) {
    let request_id = s(p, "requestId");
    let url = s(&p["request"], "url");
    let method = s(&p["request"], "method");
    let spec = {
        let mut mocks = shared.mocks.lock().unwrap();
        let hit = mocks.iter().rposition(|m| {
            m.url.is_match(&url) && m.spec.method.as_deref().is_none_or(|x| x.eq_ignore_ascii_case(&method))
        });
        hit.map(|i| {
            mocks[i].hits += 1;
            let spec = mocks[i].spec.clone();
            if spec.times.is_some_and(|t| mocks[i].hits >= t) {
                mocks.remove(i);
            }
            spec
        })
    };
    use swiftbrowse_core::request::RouteAction;
    if spec.as_ref().is_some_and(|s| s.action != RouteAction::Continue) {
        if let Some(net) = p.get("networkId").and_then(Value::as_str) {
            shared.mocked.lock().unwrap().insert(net.to_string());
            if let Some(r) = shared.requests.lock().unwrap().iter_mut().rev().find(|r| r.request_id == net) {
                r.mocked = true;
            }
        }
    }
    let (cdp, session) = (cdp.clone(), session.to_string());
    tokio::spawn(async move {
        let (method, params) = match spec {
            None => ("Fetch.continueRequest", json!({ "requestId": request_id })),
            Some(spec) => {
                if let Some(ms) = spec.delay_ms {
                    tokio::time::sleep(Duration::from_millis(ms.min(60_000))).await;
                }
                match spec.action {
                    RouteAction::Continue => ("Fetch.continueRequest", json!({ "requestId": request_id })),
                    RouteAction::Abort => (
                        "Fetch.failRequest",
                        json!({ "requestId": request_id, "errorReason": spec.error.as_deref().unwrap_or("Failed") }),
                    ),
                    RouteAction::Fulfill => {
                        let mut headers: Vec<Value> =
                            spec.headers.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect();
                        let has = |h: &[Value], n: &str| h.iter().any(|x| s(x, "name").eq_ignore_ascii_case(n));
                        let body = match (&spec.json, &spec.body) {
                            (Some(j), _) => {
                                if !has(&headers, "content-type") {
                                    headers.push(json!({ "name": "content-type", "value": "application/json" }));
                                }
                                j.to_string()
                            }
                            (None, Some(b)) => b.clone(),
                            (None, None) => String::new(),
                        };
                        if !has(&headers, "content-type") {
                            headers.push(json!({ "name": "content-type", "value": "text/plain; charset=utf-8" }));
                        }
                        let body = base64::engine::general_purpose::STANDARD.encode(body);
                        (
                            "Fetch.fulfillRequest",
                            json!({ "requestId": request_id, "responseCode": spec.status.unwrap_or(200), "responseHeaders": headers, "body": body }),
                        )
                    }
                }
            }
        };
        let _ = cdp.send(method, params, Some(&session), CMD_TIMEOUT).await;
    });
}

async fn pump_events(
    mut events: tokio::sync::mpsc::UnboundedReceiver<Event>,
    cdp: Cdp,
    session: String,
    state: Arc<Mutex<State>>,
    lifecycle: watch::Sender<u64>,
    shared: Arc<Shared>,
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
                if c["auxData"]["isDefault"].as_bool() == Some(true) {
                    if let Some(id) = c["id"].as_i64() {
                        st.frame_contexts.insert(s(&c["auxData"], "frameId"), id);
                    }
                }
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
                let id = p["executionContextId"].as_i64();
                if st.world == id {
                    st.world = None;
                }
                st.frame_contexts.retain(|_, c| Some(*c) != id);
            }
            "Runtime.executionContextsCleared" => {
                st.world = None;
                st.frame_contexts.clear();
            }
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
            "Fetch.requestPaused" => answer_paused(p, &cdp, &session, &shared),
            "Page.screencastFrame" => {
                if let Some(frames) = shared.screencast.lock().unwrap().as_mut() {
                    *shared.last_frame.lock().unwrap() = Some(Instant::now());
                    shared.streamed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if let Ok(jpeg) = base64::engine::general_purpose::STANDARD.decode(s(p, "data")) {
                        if frames.len() >= video::MAX_FRAMES {
                            frames.pop_front();
                        }
                        let timestamp = p["metadata"]["timestamp"].as_f64().unwrap_or(0.0);
                        frames.push_back(video::Frame { timestamp, jpeg });
                    }
                }
                // Chromium sends the next frame only after this one is acknowledged.
                let (cdp, session, ack) = (cdp.clone(), session.clone(), p["sessionId"].clone());
                tokio::spawn(async move {
                    let _ = cdp
                        .send("Page.screencastFrameAck", json!({ "sessionId": ack }), Some(&session), CMD_TIMEOUT)
                        .await;
                });
            }
            "Network.requestWillBeSent" => {
                let request_id = s(p, "requestId");
                let mocked = shared.mocked.lock().unwrap().contains(&request_id);
                let mut log = shared.requests.lock().unwrap();
                if log.len() >= MAX_REQUESTS {
                    log.pop_front();
                }
                log.push_back(RequestRecord {
                    method: s(&p["request"], "method"),
                    url: s(&p["request"], "url"),
                    status: None,
                    resource_type: s(p, "type"),
                    mocked,
                    request_id,
                });
                drop(log);
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
                {
                    let id = s(p, "requestId");
                    let mocked = shared.mocked.lock().unwrap().contains(&id);
                    if let Some(r) = shared.requests.lock().unwrap().iter_mut().rev().find(|r| r.request_id == id) {
                        r.status = Some(status);
                        r.mocked |= mocked;
                    }
                }
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
            warnings.push(format!(
                "Chromium sandbox disabled ({}); rely on a container or VM boundary",
                launch::no_sandbox_reason().unwrap_or("by configuration")
            ));
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
                cap(names::FRAMES, Support::Full, Some("switch_frame into same-site, cross-site and nested iframes; one frame at a time")),
                cap(names::SHADOW_DOM, Support::Partial, Some("open shadow roots for semantic and text locators; CSS does not pierce")),
                if self.daemon_context.is_some() {
                    cap(names::ISOLATED_SESSIONS, Support::Partial, Some("separate browser context per session in a shared browser"))
                } else {
                    cap(names::ISOLATED_SESSIONS, Support::Full, Some("separate process and profile per session"))
                },
                cap(names::DOWNLOADS, Support::Unsupported, None),
                cap(names::UPLOADS, Support::Full, Some("upload_file, limited to directories the policy allows")),
                cap(names::VISUAL_GROUNDING, Support::Unsupported, None),
            ],
            warnings,
        }
    }

    async fn navigate(&mut self, url: &str, wait_until: WaitUntil, timeout: Duration) -> Result<NavigationInfo> {
        let started = Instant::now();
        trace_event("navigate start");
        self.leave_frames();
        let r = self.cdp.send("Page.navigate", json!({ "url": url }), Some(&self.tab.session), timeout).await?;
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
                let status = self.tab.state.lock().unwrap().doc_status.get(&loader).copied();
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
        let status = self.tab.state.lock().unwrap().doc_status.get(&loader).copied();
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
        let mut st = self.tab.state.lock().unwrap();
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
        self.worlds_at_click = self.tab.state.lock().unwrap().worlds_created;
        self.tabs_at_click = self.shared.pages.lock().unwrap().len();
        let (ox, oy) = self.frame_offset().await?;
        let (x, y) = (pt["x"].as_f64().unwrap_or(0.0) + ox, pt["y"].as_f64().unwrap_or(0.0) + oy);
        // Queue move, press and release together: waiting for each acknowledgement
        // leaves time for the page to shift the layout between press and release.
        let events = [
            json!({ "type": "mouseMoved", "x": x, "y": y, "buttons": 0 }),
            json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": 1 }),
            json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "buttons": 0, "clickCount": 1 }),
        ];
        let pending: Vec<_> = events
            .into_iter()
            .map(|e| self.cdp.enqueue("Input.dispatchMouseEvent", e, Some(&self.tab.session)))
            .collect();
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
        let (ox, oy) = self.frame_offset().await?;
        let (x, y) = (pt["x"].as_f64().unwrap_or(0.0) + ox, pt["y"].as_f64().unwrap_or(0.0) + oy);
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
        self.leave_frames();
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
        let mut params =
            json!({ "expression": expression, "returnByValue": true, "awaitPromise": true, "userGesture": true });
        // In a frame: its main world. A cross-site frame is the main frame of its own
        // session; a same-site one needs its context id.
        if let Some(level) = self.frame.last() {
            if self.shared.iframes.lock().unwrap().get(&level.frame_id) != Some(&level.session) {
                let ctx = self.tab.state.lock().unwrap().frame_contexts.get(&level.frame_id).copied();
                let ctx =
                    ctx.ok_or_else(|| CoreError::unsupported("evaluate is not available in this nested frame"))?;
                params["contextId"] = json!(ctx);
            }
        }
        let session = self.context_session().to_string();
        let r = self.cdp.send("Runtime.evaluate", params, Some(&session), timeout).await?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(js_error(ex));
        }
        Ok(r["result"].get("value").cloned().unwrap_or(Value::Null))
    }

    async fn dom_snapshot(&mut self, mark: Option<&str>) -> Result<Value> {
        self.call("domSnapshot", json!([mark])).await
    }

    async fn start_video(&mut self) -> Result<()> {
        use std::sync::atomic::Ordering::Relaxed;
        *self.shared.screencast.lock().unwrap() = Some(VecDeque::new());
        *self.shared.video_session.lock().unwrap() = self.tab.session.clone();
        *self.shared.last_frame.lock().unwrap() = None;
        self.shared.streamed.store(0, Relaxed);
        self.shared.filled.store(0, Relaxed);
        self.send("Page.startScreencast", self.screencast_params()).await?;
        // The screencast only sends a frame when the page repaints, and some
        // platforms (seen on macOS) send none at all. Whenever it has been quiet
        // for a while, capture the page instead, which also forces a repaint.
        let (cdp, shared) = (self.cdp.clone(), self.shared.clone());
        tokio::spawn(async move {
            const QUIET: Duration = Duration::from_millis(250);
            loop {
                tokio::time::sleep(QUIET).await;
                if shared.screencast.lock().unwrap().is_none() {
                    break;
                }
                if shared.last_frame.lock().unwrap().is_some_and(|t| t.elapsed() < QUIET) {
                    continue;
                }
                let session = shared.video_session.lock().unwrap().clone();
                let params = json!({ "format": "jpeg", "quality": 70 });
                let Ok(r) = cdp.send("Page.captureScreenshot", params, Some(&session), Duration::from_secs(5)).await
                else {
                    continue;
                };
                let Ok(jpeg) = base64::engine::general_purpose::STANDARD.decode(s(&r, "data")) else { continue };
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                if let Some(frames) = shared.screencast.lock().unwrap().as_mut() {
                    if frames.len() >= video::MAX_FRAMES {
                        frames.pop_front();
                    }
                    frames.push_back(video::Frame { timestamp, jpeg });
                    *shared.last_frame.lock().unwrap() = Some(Instant::now());
                    shared.filled.fetch_add(1, Relaxed);
                }
            }
        });
        Ok(())
    }

    async fn stop_video(&mut self, dir: &Path, stem: &str) -> Result<Value> {
        let _ = self.send("Page.stopScreencast", json!({})).await;
        // The screencast only sends frames when the page repaints, and some
        // platforms send none for a short run: end on a real capture of the page.
        let last = self
            .send("Page.captureScreenshot", json!({ "format": "jpeg", "quality": 70 }))
            .await
            .ok()
            .and_then(|r| base64::engine::general_purpose::STANDARD.decode(s(&r, "data")).ok());
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        if let (Some(jpeg), Some(frames)) = (last, self.shared.screencast.lock().unwrap().as_mut()) {
            frames.push_back(video::Frame { timestamp: now, jpeg });
        }
        let frames: Vec<video::Frame> = self
            .shared
            .screencast
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| CoreError::invalid("no video is being recorded; start_video first"))?
            .into();
        let end =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let (dir, stem) = (dir.to_path_buf(), stem.to_string());
        let count = frames.len();
        let saved = tokio::task::spawn_blocking(move || video::save(&frames, end, &dir, &stem))
            .await
            .map_err(|e| CoreError::backend(format!("video encoding failed: {e}")))??;
        let streamed = self.shared.streamed.load(std::sync::atomic::Ordering::Relaxed);
        let filled = self.shared.filled.load(std::sync::atomic::Ordering::Relaxed);
        let note = (streamed == 0).then(|| {
            format!("the browser streamed no frames; the video is made of {} captures of the page", filled + 1)
        });
        Ok(match saved {
            video::Recording::Video(path) => json!({
                "path": path, "format": "webm", "frames": count, "streamed": streamed, "filled": filled, "note": note,
            }),
            video::Recording::Frames { dir, count } => json!({
                "path": dir, "format": "jpeg-frames", "frames": count,
                "note": "no ffmpeg found (npx playwright install ffmpeg, or set SWIFTBROWSE_FFMPEG): kept the frames instead",
            }),
        })
    }

    async fn storage_state(&mut self) -> Result<Value> {
        let mut params = json!({});
        if let Some(ctx) = &self.daemon_context {
            params["browserContextId"] = json!(ctx);
        }
        let r = self.cdp.send("Storage.getCookies", params, None, CMD_TIMEOUT).await?;
        let cookies: Vec<Value> = r["cookies"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                json!({
                    "name": c["name"], "value": c["value"], "domain": c["domain"], "path": c["path"],
                    "expires": c["expires"], "httpOnly": c["httpOnly"], "secure": c["secure"],
                    "sameSite": c.get("sameSite").and_then(Value::as_str).unwrap_or("Lax"),
                })
            })
            .collect();
        // localStorage of every origin open in this session's tabs.
        let mut origins: Vec<Value> = Vec::new();
        let expr = "(() => { try { return { origin: location.origin, items: Object.entries(localStorage) }; } catch (e) { return null; } })()";
        for session in self.tab_sessions() {
            let params = json!({ "expression": expr, "returnByValue": true });
            let Ok(r) = self.cdp.send("Runtime.evaluate", params, Some(&session), CMD_TIMEOUT).await else { continue };
            let v = &r["result"]["value"];
            let origin = s(v, "origin");
            if !origin.starts_with("http") || origins.iter().any(|o| s(o, "origin") == origin) {
                continue;
            }
            let items: Vec<Value> = v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|kv| json!({ "name": kv[0], "value": kv[1] }))
                .collect();
            if !items.is_empty() {
                origins.push(json!({ "origin": origin, "localStorage": items }));
            }
        }
        Ok(json!({ "cookies": cookies, "origins": origins }))
    }

    async fn set_storage_state(&mut self, state: &Value) -> Result<()> {
        let cookies: Vec<Value> = state["cookies"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let mut p = json!({
                    "name": c["name"], "value": c["value"], "domain": c["domain"],
                    "path": c.get("path").cloned().unwrap_or(json!("/")),
                    "secure": c["secure"].as_bool().unwrap_or(false),
                    "httpOnly": c["httpOnly"].as_bool().unwrap_or(false),
                });
                if let Some(same) = c["sameSite"].as_str() {
                    p["sameSite"] = json!(same);
                }
                // -1 (or missing) is a session cookie.
                if let Some(exp) = c["expires"].as_f64().filter(|e| *e > 0.0) {
                    p["expires"] = json!(exp);
                }
                p
            })
            .collect();
        if !cookies.is_empty() {
            let mut params = json!({ "cookies": cookies });
            if let Some(ctx) = &self.daemon_context {
                params["browserContextId"] = json!(ctx);
            }
            self.cdp.send("Storage.setCookies", params, None, CMD_TIMEOUT).await?;
        }
        // localStorage can only be written from a document of its origin: load an
        // intercepted blank page there (never reaching the network), write, move on.
        let mut seeded = false;
        for o in state["origins"].as_array().into_iter().flatten() {
            let origin = s(o, "origin");
            let items = o["localStorage"].as_array().cloned().unwrap_or_default();
            if !origin.starts_with("http") || items.is_empty() {
                continue;
            }
            let seed = format!("{}/__swiftbrowse_seed__", origin.trim_end_matches('/'));
            let route = swiftbrowse_core::request::RouteArgs {
                url_regex: Some(format!("^{}$", regex::escape(&seed))),
                body: Some("<!doctype html><title></title>".into()),
                headers: [("content-type".to_string(), "text/html".to_string())].into(),
                times: Some(1),
                ..Default::default()
            };
            self.add_route(route).await?;
            self.navigate_bare(&seed).await?;
            let pairs: Vec<(Value, Value)> = items.iter().map(|kv| (kv["name"].clone(), kv["value"].clone())).collect();
            let expr = format!("for (const [k, v] of {}) localStorage.setItem(k, v); true", json!(pairs));
            self.send("Runtime.evaluate", json!({ "expression": expr })).await?;
            seeded = true;
        }
        if seeded {
            self.navigate_bare("about:blank").await?;
            self.shared.requests.lock().unwrap().retain(|r| !r.url.contains("/__swiftbrowse_seed__"));
            if self.shared.mocks.lock().unwrap().is_empty() {
                for session in self.tab_sessions() {
                    let _ = self.cdp.send("Fetch.disable", json!({}), Some(&session), CMD_TIMEOUT).await;
                }
            }
        }
        Ok(())
    }

    async fn add_route(&mut self, route: swiftbrowse_core::request::RouteArgs) -> Result<String> {
        let url = route.matcher()?;
        let n = self.shared.next_mock.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let id = format!("r{n}");
        let first = {
            let mut mocks = self.shared.mocks.lock().unwrap();
            mocks.push(Mock { id: id.clone(), spec: route, url, hits: 0 });
            mocks.len() == 1
        };
        if first {
            // Interception costs a round trip per request: only while routes exist.
            for session in self.tab_sessions() {
                self.cdp.send("Fetch.enable", fetch_patterns(), Some(&session), CMD_TIMEOUT).await?;
            }
        }
        Ok(id)
    }

    async fn remove_routes(&mut self, id: Option<&str>) -> Result<usize> {
        let (removed, empty) = {
            let mut mocks = self.shared.mocks.lock().unwrap();
            let before = mocks.len();
            mocks.retain(|m| id.is_some_and(|id| m.id != id));
            (before - mocks.len(), mocks.is_empty())
        };
        if empty && removed > 0 {
            for session in self.tab_sessions() {
                let _ = self.cdp.send("Fetch.disable", json!({}), Some(&session), CMD_TIMEOUT).await;
            }
        }
        Ok(removed)
    }

    async fn routes(&mut self) -> Result<Vec<Value>> {
        Ok(self
            .shared
            .mocks
            .lock()
            .unwrap()
            .iter()
            .map(|m| {
                json!({
                    "id": m.id, "url": m.spec.url, "url_regex": m.spec.url_regex, "method": m.spec.method,
                    "action": m.spec.action, "hits": m.hits, "times": m.spec.times,
                })
            })
            .collect())
    }

    async fn requests(&mut self) -> Result<Vec<RequestRecord>> {
        Ok(self.shared.requests.lock().unwrap().iter().cloned().collect())
    }

    async fn upload_files(&mut self, ref_id: &str, files: &[PathBuf]) -> Result<Vec<String>> {
        let object = self.element_object(ref_id).await?;
        let files: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
        let session = self.context_session().to_string();
        // Fires `input` and `change` like a user's choice in the file dialog.
        self.cdp
            .send("DOM.setFileInputFiles", json!({ "files": files, "objectId": object }), Some(&session), CMD_TIMEOUT)
            .await?;
        let v = self.call("fileNames", json!([ref_id])).await?;
        serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()))
    }

    fn tab_count(&self) -> usize {
        self.shared.pages.lock().unwrap().len().max(1)
    }

    async fn tabs(&mut self) -> Result<Vec<TabInfo>> {
        let mut pages = self.shared.pages.lock().unwrap().clone();
        // Target info can lag behind (its title starts as the URL): read URL and
        // title from the pages this session is attached to.
        let active = self.page_state_of_tab().await.ok();
        for tab in &self.others {
            let expr =
                json!({ "expression": "({ url: location.href, title: document.title })", "returnByValue": true });
            let Ok(r) = self.cdp.send("Runtime.evaluate", expr, Some(&tab.session), Duration::from_secs(2)).await
            else {
                continue;
            };
            if let (Some(p), Ok(st)) = (
                pages.iter_mut().find(|p| p.target_id == tab.target_id),
                serde_json::from_value::<PageState>(r["result"]["value"].clone()),
            ) {
                p.url = st.url;
                p.title = st.title;
            }
        }
        Ok(pages
            .iter()
            .enumerate()
            .map(|(index, p)| {
                let is_active = p.target_id == self.tab.target_id;
                let (url, title) = match (&active, is_active) {
                    (Some(st), true) => (st.url.clone(), st.title.clone()),
                    _ => (p.url.clone(), p.title.clone()),
                };
                let opener = p.opener.as_ref().and_then(|o| pages.iter().position(|q| &q.target_id == o));
                TabInfo { index, url, title, active: is_active, opener }
            })
            .collect())
    }

    async fn new_tab(&mut self) -> Result<()> {
        let mut params = json!({ "url": "about:blank" });
        if let Some(ctx) = &self.daemon_context {
            params["browserContextId"] = json!(ctx);
        }
        let r = self.cdp.send("Target.createTarget", params, None, CMD_TIMEOUT).await?;
        let target = s(&r, "targetId");
        {
            let mut pages = self.shared.pages.lock().unwrap();
            if !pages.iter().any(|p| p.target_id == target) {
                pages.push(TargetSeen { target_id: target.clone(), url: "about:blank".into(), ..Default::default() });
            }
        }
        self.activate(&target).await
    }

    async fn switch_tab(&mut self, index: usize) -> Result<()> {
        let target = self.shared.pages.lock().unwrap().get(index).map(|p| p.target_id.clone());
        let target = target.ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("no tab {index}")))?;
        self.activate(&target).await
    }

    async fn close_tab(&mut self, index: usize) -> Result<()> {
        let (target, opener, next) = {
            let pages = self.shared.pages.lock().unwrap();
            let p = pages.get(index).ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("no tab {index}")))?;
            let next = pages.iter().rev().find(|q| q.target_id != p.target_id).map(|q| q.target_id.clone());
            (p.target_id.clone(), p.opener.clone(), next)
        };
        if self.tab.target_id == target {
            let alive = |id: &String| self.shared.pages.lock().unwrap().iter().any(|p| &p.target_id == id);
            let to = opener.filter(alive).or(next).ok_or_else(|| CoreError::invalid("cannot close the last tab"))?;
            self.activate(&to).await?;
        }
        if let Some(i) = self.others.iter().position(|t| t.target_id == target) {
            let tab = self.others.remove(i);
            tab.pump.abort();
            self.shared.routes.lock().unwrap().remove(&tab.session);
        }
        self.cdp.send("Target.closeTarget", json!({ "targetId": target }), None, CMD_TIMEOUT).await?;
        self.shared.pages.lock().unwrap().retain(|p| p.target_id != target);
        Ok(())
    }

    async fn frames(&mut self) -> Result<Vec<FrameInfo>> {
        fn walk(node: &Value, parent: Option<String>, out: &mut Vec<FrameInfo>) {
            let f = &node["frame"];
            let id = s(f, "id");
            out.push(FrameInfo {
                id: id.clone(),
                name: s(f, "name"),
                url: format!("{}{}", s(f, "url"), s(f, "urlFragment")),
                parent: f.get("parentId").and_then(Value::as_str).map(str::to_string).or(parent),
                out_of_process: false,
            });
            for child in node["childFrames"].as_array().into_iter().flatten() {
                walk(child, Some(id.clone()), out);
            }
        }
        let tree = self.send("Page.getFrameTree", json!({})).await?;
        let mut out = Vec::new();
        walk(&tree["frameTree"], None, &mut out);
        // Cross-site iframes are documents in other processes: ask their sessions.
        let iframes: Vec<(String, String)> =
            self.shared.iframes.lock().unwrap().iter().map(|(f, s)| (f.clone(), s.clone())).collect();
        for (frame_id, session) in iframes {
            let Ok(sub) = self.cdp.send("Page.getFrameTree", json!({}), Some(&session), CMD_TIMEOUT).await else {
                continue;
            };
            let mut found = Vec::new();
            walk(&sub["frameTree"], None, &mut found);
            for mut f in found {
                f.out_of_process = true;
                match out.iter_mut().find(|o| o.id == f.id) {
                    Some(o) => {
                        o.out_of_process = true;
                        if o.url.is_empty() {
                            o.url = f.url;
                        }
                        if o.name.is_empty() {
                            o.name = f.name;
                        }
                    }
                    None => {
                        if f.id == frame_id && f.parent.is_none() {
                            f.parent = Some(self.tab.target_id.clone());
                        }
                        out.push(f);
                    }
                }
            }
        }
        Ok(out)
    }

    async fn enter_frame(&mut self, frame: Option<FrameRef>) -> Result<FrameInfo> {
        let frame_id = match frame {
            None => {
                self.leave_frames();
                let page = self.page_state().await?;
                return Ok(FrameInfo {
                    id: self.tab.target_id.clone(),
                    name: String::new(),
                    url: page.url,
                    parent: None,
                    out_of_process: false,
                });
            }
            Some(FrameRef::Element(ref_id)) => {
                let object = self.element_object(&ref_id).await?;
                let node = self
                    .cdp
                    .send("DOM.describeNode", json!({ "objectId": object }), Some(self.context_session()), CMD_TIMEOUT)
                    .await?;
                let id = s(&node["node"], "frameId");
                if id.is_empty() {
                    return Err(CoreError::new(ErrorKind::NotActionable, format!("{ref_id} has no frame loaded yet")));
                }
                id
            }
            Some(FrameRef::Id(id)) => id,
        };
        // The chain of frames from the main frame down to this one.
        let frames = self.frames().await?;
        let mut chain = vec![frame_id.clone()];
        while let Some(parent) = frames.iter().find(|f| f.id == *chain.last().unwrap()).and_then(|f| f.parent.clone()) {
            if parent == self.tab.target_id {
                break;
            }
            chain.push(parent);
        }
        if frames.iter().all(|f| f.id != frame_id) {
            return Err(CoreError::new(ErrorKind::NotFound, "that frame is not in this tab (any more)"));
        }
        chain.reverse();
        let mut levels: Vec<FrameLevel> = Vec::new();
        for id in chain {
            let parent_session =
                levels.last().map(|l: &FrameLevel| l.session.clone()).unwrap_or(self.tab.session.clone());
            let oopif = self.shared.iframes.lock().unwrap().get(&id).cloned();
            let owner = self
                .cdp
                .send("DOM.getFrameOwner", json!({ "frameId": id }), Some(&parent_session), CMD_TIMEOUT)
                .await
                .map_err(|e| CoreError::new(ErrorKind::NotActionable, format!("frame not ready: {}", e.message)))?;
            let owner =
                owner["backendNodeId"].as_i64().ok_or_else(|| CoreError::backend("frame has no owner element"))?;
            if let Some(session) = &oopif {
                // A cross-site frame's session needs Page and Runtime before it can host a world.
                for m in ["Page.enable", "Runtime.enable"] {
                    let _ = self.cdp.send(m, json!({}), Some(session), CMD_TIMEOUT).await;
                }
            }
            levels.push(FrameLevel {
                frame_id: id,
                session: oopif.unwrap_or_else(|| parent_session.clone()),
                owner,
                owner_session: parent_session,
            });
        }
        self.frame = levels;
        *self.frame_world.lock().unwrap() = None;
        let state = match self.page_state().await {
            Ok(st) => st,
            Err(e) => {
                self.leave_frames();
                return Err(CoreError::new(ErrorKind::NotActionable, format!("frame not ready: {}", e.message)));
            }
        };
        let info = frames.into_iter().find(|f| f.id == frame_id);
        Ok(FrameInfo {
            id: frame_id,
            name: info.as_ref().map(|f| f.name.clone()).unwrap_or_default(),
            url: state.url,
            parent: info.as_ref().and_then(|f| f.parent.clone()),
            out_of_process: info.is_some_and(|f| f.out_of_process),
        })
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
        let jpeg = path.extension().is_some_and(|e| e == "jpg" || e == "jpeg");
        let mut params = if jpeg { json!({ "format": "jpeg", "quality": 70 }) } else { json!({ "format": "png" }) };
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
        let new_document = |me: &Self| me.tab.state.lock().unwrap().worlds_created > me.worlds_at_click;
        // A document replaced since the click is the strongest possible effect; the
        // new document's helper knows nothing about the click, so ask the state first.
        if new_document(self) {
            return Ok(Some(vec!["navigated".into()]));
        }
        // A click that opens a tab (popup, target=_blank) leaves this page as it
        // was: the new tab is the effect, so stop waiting the moment it appears.
        let shared = self.shared.clone();
        let tabs_before = self.tabs_at_click;
        let opened = async move {
            while shared.pages.lock().unwrap().len() <= tabs_before {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        let effect = tokio::select! {
            r = self.call("clickEffect", json!([max.as_millis() as u64])) => r,
            _ = opened => return Ok(Some(vec!["opened_tab".into()])),
        };
        match effect {
            Ok(_) if new_document(self) => Ok(Some(vec!["navigated".into()])),
            Ok(v) => Ok(serde_json::from_value(v).ok().flatten()),
            Err(e) if context_gone(&e) || new_document(self) => Ok(Some(vec!["navigated".into()])),
            Err(e) => Err(e),
        }
    }

    async fn console_errors(&mut self) -> Result<Vec<ConsoleEntry>> {
        Ok(self.tab.state.lock().unwrap().console.iter().cloned().collect())
    }

    async fn network_failures(&mut self) -> Result<Vec<NetworkFailure>> {
        Ok(self.tab.state.lock().unwrap().network.iter().cloned().collect())
    }

    async fn dialogs(&mut self) -> Result<Vec<DialogEvent>> {
        Ok(self.tab.state.lock().unwrap().dialogs.iter().cloned().collect())
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
        self.router.abort();
        self.tab.pump.abort();
        for t in &self.others {
            t.pump.abort();
        }
        Ok(())
    }
}
