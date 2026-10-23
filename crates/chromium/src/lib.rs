//! Chromium backend: one browser process and one isolated browser context per
//! session, driven over the Chrome DevTools Protocol.

mod cdp;
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

#[derive(Debug, Clone, Default)]
pub struct ChromiumFactory {
    /// Browser binary; discovered with [`launch::discover`] when `None`.
    pub executable: Option<PathBuf>,
    /// Force `--no-sandbox` on or off; `None` means only when running as root.
    pub no_sandbox: Option<bool>,
    pub extra_args: Vec<String>,
}

#[async_trait]
impl BackendFactory for ChromiumFactory {
    fn name(&self) -> &str {
        "chromium"
    }

    async fn launch(&self, options: &SessionOptions) -> Result<Box<dyn Backend>> {
        let exe = self.executable.clone().or_else(launch::discover).ok_or_else(|| {
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
    helper_in: Option<i64>,
    lifecycle: HashSet<(String, String)>,
    doc_status: HashMap<String, u16>,
    requests: HashMap<String, (String, String)>,
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
    child: tokio::process::Child,
    profile: Option<tempfile::TempDir>,
    pump: tokio::task::JoinHandle<()>,
    version: Option<String>,
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
        let (cdp, events) = Cdp::connect(&launched.ws_url).await?;
        trace("devtools connected");

        // Each session already owns a browser process and a throwaway profile, so
        // the initial tab is isolated; reuse it rather than creating a context.
        let (version, targets) = tokio::join!(
            cdp.send("Browser.getVersion", json!({}), None, CMD_TIMEOUT),
            cdp.send("Target.getTargets", json!({}), None, CMD_TIMEOUT),
        );
        let version = version.ok().map(|v| s(&v, "product"));
        let existing = targets?["targetInfos"]
            .as_array()
            .and_then(|t| t.iter().find(|t| s(t, "type") == "page").map(|t| s(t, "targetId")));
        let target_id = match existing {
            Some(id) => id,
            None => s(
                &cdp.send("Target.createTarget", json!({ "url": "about:blank" }), None, CMD_TIMEOUT).await?,
                "targetId",
            ),
        };
        let attached = cdp
            .send("Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }), None, CMD_TIMEOUT)
            .await?;
        let session = s(&attached, "sessionId");
        trace("page target attached");

        let state = Arc::new(Mutex::new(State::default()));
        let (lc_tx, lc_rx) = watch::channel(0u64);
        let pump = tokio::spawn(pump_events(events, cdp.clone(), session.clone(), state.clone(), lc_tx));

        // Independent setup commands go out together instead of one round trip each.
        let sid = Some(session.as_str());
        let setup = [
            ("Page.getFrameTree", json!({})),
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
                json!({ "width": options.viewport.0, "height": options.viewport.1, "deviceScaleFactor": 1, "mobile": false }),
            ),
        ];
        let cdp_ref = &cdp;
        let replies = futures_util::future::join_all(setup.into_iter().map(|(m, p)| async move {
            let r = cdp_ref.send(m, p, sid, CMD_TIMEOUT).await;
            trace(m);
            r
        }))
        .await;
        let mut replies = replies.into_iter().collect::<Result<Vec<_>>>()?.into_iter();
        let tree = replies.next().unwrap_or_default();
        state.lock().unwrap().main_frame = s(&tree["frameTree"]["frame"], "id");
        trace("page domains enabled");

        Ok(Self {
            cdp,
            session,
            state,
            lifecycle: lc_rx,
            child: launched.child,
            profile: Some(launched.profile),
            pump,
            version,
            headless: options.headless,
            no_sandbox,
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
            let call = format!("globalThis.__sb.{func}(...{args})");
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
                    self.state.lock().unwrap().helper_in = Some(ctx);
                    return Ok(v["result"].get("value").cloned().unwrap_or(Value::Null));
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
    async fn load_in_page(&mut self, worlds_before: u64, deadline: tokio::time::Instant) -> Result<Option<PageState>> {
        loop {
            self.lifecycle.borrow_and_update();
            {
                let st = self.state.lock().unwrap();
                if st.worlds_created > worlds_before && st.world.is_some() {
                    break;
                }
            }
            match tokio::time::timeout_at(deadline, self.lifecycle.changed()).await {
                Ok(Ok(())) => continue,
                Ok(Err(_)) => return Err(CoreError::backend("browser connection closed")),
                Err(_) => return Ok(None),
            }
        }
        match tokio::time::timeout_at(deadline, self.call("loaded", json!([]))).await {
            Ok(Ok(v)) => Ok(serde_json::from_value(v).ok()),
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
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.start_kill();
            let deadline = Instant::now() + Duration::from_secs(2);
            while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
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
                    lifecycle.send_modify(|n| *n += 1);
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
            }
            "Network.loadingFailed" => {
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
        if self.no_sandbox {
            warnings.push(
                "Chromium sandbox disabled (running as root or SWIFTBROWSE_NO_SANDBOX=1); use a container boundary"
                    .into(),
            );
        }
        CapabilityReport {
            backend: "chromium".into(),
            engine: "chromium".into(),
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
                cap(names::ISOLATED_SESSIONS, Support::Full, Some("separate process and profile per session")),
                cap(names::DOWNLOADS, Support::Unsupported, None),
                cap(names::UPLOADS, Support::Unsupported, None),
                cap(names::VISUAL_GROUNDING, Support::Unsupported, None),
            ],
            warnings,
        }
    }

    async fn navigate(&mut self, url: &str, wait_until: WaitUntil, timeout: Duration) -> Result<NavigationInfo> {
        let started = Instant::now();
        let worlds_before = self.state.lock().unwrap().worlds_created;
        let r = self.cdp.send("Page.navigate", json!({ "url": url }), Some(&self.session), timeout).await?;
        trace(started, "navigate committed");
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
            if let Some(page) = self.load_in_page(worlds_before, deadline).await? {
                trace(started, "loaded (in-page)");
                let status = self.state.lock().unwrap().doc_status.get(&loader).copied();
                return Ok(NavigationInfo { url: page.url, title: page.title, status, same_document });
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
        // Read from the page: Chromium's pushed target title still holds the URL
        // placeholder when `load` fires, so it cannot be trusted here.
        let page = self.page_state().await?;
        trace(started, "page state read");
        Ok(NavigationInfo { url: page.url, title: page.title, status, same_document })
    }

    async fn page_state(&mut self) -> Result<PageState> {
        let v = self.call("pageState", json!([])).await?;
        serde_json::from_value(v).map_err(|e| CoreError::backend(e.to_string()))
    }

    async fn snapshot(&mut self, mode: SnapshotMode, max_elements: usize) -> Result<PageSnapshot> {
        let mode = if mode == SnapshotMode::Full { "full" } else { "compact" };
        let v = self.call("snapshot", json!([mode, max_elements])).await?;
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
        let (x, y) = (pt["x"].as_f64().unwrap_or(0.0), pt["y"].as_f64().unwrap_or(0.0));
        let dispatch = async {
            self.send("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y })).await?;
            self.send(
                "Input.dispatchMouseEvent",
                json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }),
            )
            .await?;
            self.send(
                "Input.dispatchMouseEvent",
                json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }),
            )
            .await
        };
        // Input may have reached the page: never report a retryable kind from here.
        dispatch.await.map(|_| ()).map_err(|e| CoreError::backend(format!("click dispatch failed: {}", e.message)))
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
        let _ = self.cdp.send("Browser.close", json!({}), None, Duration::from_secs(3)).await;
        if tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
        self.pump.abort();
        Ok(())
    }
}
