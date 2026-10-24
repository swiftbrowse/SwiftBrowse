//! The execution engine: inspect → resolve → check policy → act → wait → verify → report.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::backend::{Backend, FrameRef};
use crate::capability::{names, CapabilityReport, Support};
use crate::error::{CoreError, ErrorKind, Result};
use crate::locator::{Locator, Target};
use crate::model::{now_ms, ElementInfo, PageSnapshot, PageState};
use crate::policy::Policy;
use crate::request::*;
use crate::result::*;

#[derive(Debug, Clone, Serialize)]
struct LogEntry {
    action_id: String,
    op: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<CoreError>,
    duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct AssertionRecord {
    action_id: String,
    name: Option<String>,
    passed: bool,
    checks: Vec<Check>,
}

#[derive(Default)]
struct Ctx {
    target: Option<ResolvedTarget>,
    verification: Option<Verification>,
    evidence: Vec<Evidence>,
    notes: Vec<String>,
    timeout_ms: Option<u64>,
    selected: Option<Vec<String>>,
}

enum Act<'a> {
    Click { approved: bool, consequential: Option<bool> },
    Fill(&'a str),
    Focus,
    Hover,
    Select(&'a str),
}

pub struct Session {
    id: String,
    backend: Box<dyn Backend>,
    policy: Arc<Policy>,
    caps: CapabilityReport,
    counter: u64,
    created_at_ms: u64,
    last_snapshot: Option<PageSnapshot>,
    log: VecDeque<LogEntry>,
    assertions: Vec<AssertionRecord>,
    evidence: Vec<Evidence>,
}

/// Condition-based polling: checks immediately, then backs off up to 100 ms
/// between checks until the deadline. Never sleeps past the deadline.
/// How long a click without `expect` waits to observe any effect.
const CLICK_EFFECT_WAIT_MS: u64 = 150;

struct Poll {
    deadline: Instant,
    attempt: usize,
}

impl Poll {
    fn new(timeout_ms: u64) -> Self {
        Self { deadline: Instant::now() + Duration::from_millis(timeout_ms), attempt: 0 }
    }

    async fn next(&mut self) -> bool {
        let now = Instant::now();
        if now >= self.deadline {
            return false;
        }
        const STEPS: [u64; 5] = [10, 20, 40, 80, 100];
        let step = Duration::from_millis(STEPS[self.attempt.min(STEPS.len() - 1)]);
        self.attempt += 1;
        tokio::time::sleep(step.min(self.deadline - now)).await;
        true
    }

    fn remaining_ms(&self) -> u64 {
        self.deadline.saturating_duration_since(Instant::now()).as_millis() as u64
    }
}

fn summarize(el: &ElementInfo) -> Value {
    json!({ "ref": el.ref_id, "tag": el.tag, "role": el.role, "name": el.name, "visible": el.visible })
}

impl Session {
    pub fn new(id: String, backend: Box<dyn Backend>, policy: Arc<Policy>) -> Self {
        let caps = backend.capabilities();
        Self {
            id,
            backend,
            policy,
            caps,
            counter: 0,
            created_at_ms: now_ms(),
            last_snapshot: None,
            log: VecDeque::new(),
            assertions: Vec::new(),
            evidence: Vec::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn capabilities(&self) -> &CapabilityReport {
        &self.caps
    }

    pub async fn close(&mut self) -> Result<()> {
        self.backend.close().await
    }

    pub async fn execute(&mut self, req: Request) -> ActionResult {
        let started_at_ms = now_ms();
        let t0 = Instant::now();
        self.counter += 1;
        let action_id = format!("{}-{}", self.id, self.counter);
        let op = req.op();
        let mut ctx = Ctx::default();

        let outcome = self.dispatch(&req, &action_id, &mut ctx).await;
        let (data, error) = match outcome {
            Ok(d) => (d, None),
            Err(e) => (None, Some(e)),
        };
        let ok = error.is_none();

        let observes_page =
            !matches!(req, Request::GetCapabilities(_) | Request::GetTestReport(_) | Request::GetPageSnapshot(_));
        // A successful navigate already read the page state; don't pay for it twice.
        let nav_page = match (&req, &data) {
            (Request::Navigate(_), Some(d)) => Some(PageState {
                url: d["url"].as_str().unwrap_or_default().to_string(),
                title: d["title"].as_str().unwrap_or_default().to_string(),
            }),
            _ => None,
        };
        let page = match nav_page {
            Some(p) => Some(p),
            None if observes_page => self.backend.page_state().await.ok(),
            None => None,
        };

        let wants_failure_evidence = matches!(
            req,
            Request::Navigate(_)
                | Request::Click(_)
                | Request::Fill(_)
                | Request::TypeText(_)
                | Request::PressKey(_)
                | Request::SelectOption(_)
                | Request::WaitFor(_)
                | Request::AssertPageState(_)
                | Request::Hover(_)
                | Request::Check(_)
        );
        let invalid_request = error.as_ref().is_some_and(|e| {
            matches!(e.kind, ErrorKind::InvalidArgument | ErrorKind::PermissionDenied | ErrorKind::ApprovalRequired)
        });
        if !ok
            && wants_failure_evidence
            && !invalid_request
            && self.policy.screenshot_on_failure
            && self.caps.support(names::SCREENSHOT) != Support::Unsupported
        {
            match self.capture(&format!("{action_id}-failure"), &action_id, false, ImageFormat::Png).await {
                Ok(ev) => ctx.evidence.push(ev),
                Err(e) => ctx.notes.push(format!("failure screenshot not captured: {}", e.message)),
            }
        }

        let duration_ms = t0.elapsed().as_millis() as u64;
        self.log.push_back(LogEntry {
            action_id: action_id.clone(),
            op: op.into(),
            ok,
            error: error.clone(),
            duration_ms,
        });
        while self.log.len() > self.policy.max_log_entries {
            self.log.pop_front();
        }

        ActionResult {
            ok,
            op: op.into(),
            action_id,
            session_id: self.id.clone(),
            target: ctx.target,
            page,
            verification: ctx.verification,
            timing: Timing { started_at_ms, duration_ms, timeout_ms: ctx.timeout_ms },
            data,
            error,
            evidence: ctx.evidence,
            notes: ctx.notes,
        }
    }

    async fn dispatch(&mut self, req: &Request, action_id: &str, ctx: &mut Ctx) -> Result<Option<Value>> {
        let p = self.policy.clone();
        match req {
            Request::Navigate(a) => {
                let url = p.check_navigation(&a.url)?;
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let nav = self.backend.navigate(url.as_str(), a.wait_until, Duration::from_millis(timeout)).await?;
                self.last_snapshot = None;
                if let Some(status) = nav.status.filter(|s| *s >= 400) {
                    ctx.notes.push(format!("page loaded with HTTP status {status}"));
                }
                if nav.still_empty {
                    ctx.notes.push(format!(
                        "page still looks empty {} ms after load; it may need wait_for or wait_until=network_idle",
                        nav.rendered_after_load_ms.unwrap_or_default()
                    ));
                } else if nav.still_changing {
                    ctx.notes.push(format!(
                        "page was still changing {} ms after load; snapshot again before relying on it",
                        nav.rendered_after_load_ms.unwrap_or_default()
                    ));
                } else if let Some(ms) = nav.rendered_after_load_ms {
                    ctx.notes.push(format!("page rendered client-side {ms} ms after load; waited for it"));
                }
                Ok(Some(serde_json::to_value(nav).unwrap_or_default()))
            }
            Request::GetPageSnapshot(a) => {
                let cap = match a.mode {
                    SnapshotMode::Compact => p.max_snapshot_elements,
                    SnapshotMode::Full => p.max_full_snapshot_elements,
                };
                let max = a.max_elements.unwrap_or(cap).min(cap);
                let snap = self.backend.snapshot(a.mode, max).await?;
                if snap.still_empty {
                    ctx.notes.push(format!(
                        "page still looks empty after waiting {} ms for it to render",
                        snap.rendered_after_load_ms.unwrap_or_default()
                    ));
                } else if snap.still_changing {
                    ctx.notes.push("page kept changing while snapshotting; snapshot again before relying on it".into());
                } else if let Some(ms) = snap.rendered_after_load_ms {
                    ctx.notes.push(format!("page was still rendering; waited {ms} ms before snapshotting"));
                }
                if snap.truncated {
                    ctx.notes.push(format!(
                        "snapshot truncated to {} of {} elements; use find_element or a full snapshot for more",
                        snap.elements.len(),
                        snap.total_elements
                    ));
                }
                let data = match (&self.last_snapshot, a.since_last, a.format) {
                    (Some(prev), true, SnapshotFormat::Text) => json!({
                        "format": "text",
                        "text": snap.diff_from(prev).to_text(&snap),
                    }),
                    (None, _, SnapshotFormat::Text) | (Some(_), false, SnapshotFormat::Text) => {
                        if a.since_last {
                            ctx.notes.push("no previous snapshot in this document; returned a full snapshot".into());
                        }
                        json!({
                            "format": "text",
                            "text": snap.to_text(),
                            "total_elements": snap.total_elements,
                            "truncated": snap.truncated,
                        })
                    }
                    (Some(prev), true, SnapshotFormat::Json) => json!({
                        "url": snap.url,
                        "title": snap.title,
                        "diff": snap.diff_from(prev),
                        "messages": snap.messages,
                    }),
                    (_, _, SnapshotFormat::Json) => {
                        if a.since_last {
                            ctx.notes.push("no previous snapshot in this document; returned a full snapshot".into());
                        }
                        serde_json::to_value(&snap).unwrap_or_default()
                    }
                };
                self.last_snapshot = Some(snap);
                Ok(Some(data))
            }
            Request::GetAccessibilityTree(a) => {
                let max = a.max_nodes.unwrap_or(p.max_ax_nodes).min(p.max_ax_nodes);
                Ok(Some(self.backend.accessibility_tree(max).await?))
            }
            Request::FindElement(a) => {
                a.target.locator.validate()?;
                if a.all {
                    let (matches, strategy) = self.query_all(&a.target.locator).await?;
                    return Ok(Some(json!({ "strategy": strategy, "count": matches.len(), "matches": matches })));
                }
                let resolved = self.resolve(&a.target).await?;
                let data = serde_json::to_value(&resolved.element).unwrap_or_default();
                ctx.target = Some(resolved);
                Ok(Some(data))
            }
            Request::Click(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                let act = Act::Click { approved: a.approved, consequential: a.consequential };
                self.act(&a.target, act, &mut poll, ctx).await?;
                if a.expect.is_none() {
                    // No expected outcome: at least tell the agent whether anything happened.
                    let effect =
                        self.backend.click_effect(Duration::from_millis(CLICK_EFFECT_WAIT_MS)).await.ok().flatten();
                    self.verify_expect(None, &mut poll, ctx, "click").await?;
                    if let (Some(kinds), Some(v)) = (&effect, ctx.verification.as_mut()) {
                        v.note = Some(if kinds.is_empty() {
                            format!(
                                "click was dispatched but nothing observable changed within {CLICK_EFFECT_WAIT_MS} ms; \
                                 the page may not be interactive yet. Re-check before acting on it"
                            )
                        } else {
                            format!(
                                "click was dispatched; observed: {}. Pass `expect` to confirm the intended outcome",
                                kinds.join(", ")
                            )
                        });
                    }
                    return Ok(effect.map(|k| json!({ "effect": k })));
                }
                self.verify_expect(a.expect.as_ref(), &mut poll, ctx, "click").await?;
                Ok(None)
            }
            Request::Hover(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                self.act(&a.target, Act::Hover, &mut poll, ctx).await?;
                self.verify_expect(a.expect.as_ref(), &mut poll, ctx, "hover").await?;
                Ok(None)
            }
            Request::Check(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                let want = a.checked;
                let current = loop {
                    match self.resolve(&a.target).await {
                        Ok(r) => break r.element.checked,
                        Err(e) if e.recoverable && poll.next().await => continue,
                        Err(e) => return Err(e),
                    }
                };
                if current.is_none() {
                    return Err(CoreError::invalid("check needs a checkbox, radio or element with aria-checked"));
                }
                let mut clicked = false;
                if current != Some(want) {
                    let act = Act::Click { approved: false, consequential: Some(false) };
                    self.act(&a.target, act, &mut poll, ctx).await?;
                    clicked = true;
                }
                let cond = Condition::Checked { target: a.target.clone(), checked: want };
                self.verify_expect(Some(&cond), &mut poll, ctx, "check").await?;
                Ok(Some(json!({ "checked": want, "changed": clicked })))
            }
            Request::Fill(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                let ref_id = self.act(&a.target, Act::Fill(&a.value), &mut poll, ctx).await?;
                let actual = self.backend.read_value(&ref_id).await?;
                let passed = actual.as_deref() == Some(a.value.as_str());
                let sensitive =
                    ctx.target.as_ref().is_some_and(|t| t.element.input_type.as_deref() == Some("password"));
                let observed = if sensitive { json!("<redacted>") } else { json!(actual) };
                ctx.verification = Some(Verification {
                    status: if passed { VerificationStatus::Passed } else { VerificationStatus::Failed },
                    checks: vec![Check {
                        condition: json!({ "type": "value_equals", "target": { "by": "ref", "ref": ref_id } }),
                        passed,
                        observed,
                    }],
                    note: None,
                });
                if !passed {
                    return Err(CoreError::new(ErrorKind::VerificationFailed, "field value after fill does not match"));
                }
                Ok(None)
            }
            Request::TypeText(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                if let Some(t) = &a.target {
                    let mut poll = Poll::new(timeout);
                    self.act(t, Act::Focus, &mut poll, ctx).await?;
                }
                self.backend.type_text(&a.text).await?;
                ctx.verification = Some(not_checked("text was typed; read the field or use fill to verify the value"));
                Ok(None)
            }
            Request::PressKey(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                if let Some(t) = &a.target {
                    self.act(t, Act::Focus, &mut poll, ctx).await?;
                }
                self.backend.press_key(&a.key).await?;
                self.verify_expect(a.expect.as_ref(), &mut poll, ctx, "key press").await?;
                Ok(None)
            }
            Request::SelectOption(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                let ref_id = self.act(&a.target, Act::Select(&a.option), &mut poll, ctx).await?;
                let selected = ctx.selected.take().unwrap_or_default();
                let current = self.backend.read_value(&ref_id).await?;
                let passed = current.as_ref().is_some_and(|v| selected.contains(v));
                ctx.verification = Some(Verification {
                    status: if passed { VerificationStatus::Passed } else { VerificationStatus::Failed },
                    checks: vec![Check {
                        condition: json!({ "type": "option_selected", "option": a.option }),
                        passed,
                        observed: json!({ "value": current, "selected": selected }),
                    }],
                    note: None,
                });
                if !passed {
                    return Err(CoreError::new(ErrorKind::VerificationFailed, "selected option did not stick"));
                }
                Ok(Some(json!({ "selected": selected })))
            }
            Request::WaitFor(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                let (passed, checks) = self.wait_all(std::slice::from_ref(&a.condition), &mut poll).await?;
                if !passed {
                    return Err(CoreError::new(ErrorKind::Timeout, format!("condition not met within {timeout} ms"))
                        .with_details(json!({ "checks": checks })));
                }
                Ok(Some(json!({ "checks": checks })))
            }
            Request::GoBack(a) | Request::GoForward(a) | Request::Reload(a) => {
                let delta = match req {
                    Request::GoBack(_) => -1,
                    Request::GoForward(_) => 1,
                    _ => 0,
                };
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let nav = self.backend.history(delta, Duration::from_millis(timeout)).await?;
                p.check_navigation(&nav.url).map_err(|e| {
                    CoreError::new(
                        ErrorKind::PermissionDenied,
                        format!("history led outside the policy: {}", e.message),
                    )
                })?;
                self.last_snapshot = None;
                Ok(Some(serde_json::to_value(nav).unwrap_or_default()))
            }
            Request::Evaluate(a) => {
                if !p.allow_evaluate {
                    return Err(CoreError::new(
                        ErrorKind::PermissionDenied,
                        "evaluate is disabled by policy (enabled in the test runner; MCP needs --allow-evaluate)",
                    ));
                }
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let value = self.backend.evaluate(&a.expression, Duration::from_millis(timeout)).await?;
                ctx.verification = Some(not_checked("script ran; its result is returned as data"));
                Ok(Some(json!({ "value": value })))
            }
            Request::UploadFile(a) => {
                let files = a.files.iter().map(|f| p.check_upload(f)).collect::<Result<Vec<_>>>()?;
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                a.target.locator.validate()?;
                let el = loop {
                    match self.resolve(&a.target).await {
                        Ok(r) => break r,
                        Err(e) if e.recoverable && poll.next().await => continue,
                        Err(e) => return Err(e),
                    }
                };
                if el.element.input_type.as_deref() != Some("file") {
                    return Err(CoreError::invalid(format!(
                        "{} is not an <input type=file>; target the file input itself",
                        el.element.ref_id
                    )));
                }
                let ref_id = el.element.ref_id.clone();
                ctx.target = Some(el);
                let selected = self.backend.upload_files(&ref_id, &files).await?;
                let wanted: Vec<String> = files
                    .iter()
                    .map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                    .collect();
                let passed = selected == wanted;
                ctx.verification = Some(Verification {
                    status: if passed { VerificationStatus::Passed } else { VerificationStatus::Failed },
                    checks: vec![Check {
                        condition: json!({ "type": "files_selected", "files": wanted }),
                        passed,
                        observed: json!({ "files": selected }),
                    }],
                    note: None,
                });
                if !passed {
                    return Err(CoreError::new(ErrorKind::VerificationFailed, "the input did not take the files"));
                }
                Ok(Some(json!({ "files": selected })))
            }
            Request::StartVideo(_) => {
                self.backend.start_video().await?;
                Ok(None)
            }
            Request::StopVideo(a) => {
                let stem = a.name.clone().unwrap_or_else(|| "video".into());
                // Validates the name like any evidence file.
                p.evidence_path(&stem, "webm")?;
                let saved = self.backend.stop_video(&p.evidence_dir, &stem).await?;
                if let Some(path) = saved["path"].as_str() {
                    let ev = Evidence { kind: "video".into(), path: path.into(), action_id: action_id.into() };
                    self.evidence.push(ev.clone());
                    ctx.evidence.push(ev);
                }
                Ok(Some(saved))
            }
            Request::SaveStorageState(a) => {
                let path = p.state_path(&a.name)?;
                let state = self.backend.storage_state().await?;
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| CoreError::backend(format!("cannot create {}: {e}", dir.display())))?;
                }
                // Session cookies are credentials: readable by this user only.
                write_private(&path, &serde_json::to_vec_pretty(&state).unwrap_or_default())
                    .map_err(|e| CoreError::backend(format!("cannot write {}: {e}", path.display())))?;
                let count = |k: &str| state[k].as_array().map_or(0, Vec::len);
                Ok(Some(json!({ "path": path, "cookies": count("cookies"), "origins": count("origins") })))
            }
            Request::LoadStorageState(a) => {
                let path = p.state_path(&a.name)?;
                let text = std::fs::read_to_string(&path).map_err(|_| {
                    CoreError::new(
                        ErrorKind::NotFound,
                        format!("no saved state {:?}; save_storage_state first", a.name),
                    )
                })?;
                let state: Value = serde_json::from_str(&text)
                    .map_err(|_| CoreError::invalid(format!("saved state {:?} is not valid JSON", a.name)))?;
                self.backend.set_storage_state(&state).await?;
                self.last_snapshot = None;
                let count = |k: &str| state[k].as_array().map_or(0, Vec::len);
                Ok(Some(json!({ "cookies": count("cookies"), "origins": count("origins") })))
            }
            Request::Route(a) => {
                a.matcher()?;
                if a.action == RouteAction::Fulfill && a.body.is_some() && a.json.is_some() {
                    return Err(CoreError::invalid("route takes body or json, not both"));
                }
                let id = self.backend.add_route(a.clone()).await?;
                Ok(Some(json!({ "id": id })))
            }
            Request::Unroute(a) => {
                let removed = self.backend.remove_routes(a.id.as_deref()).await?;
                if a.id.is_some() && removed == 0 {
                    return Err(CoreError::new(ErrorKind::NotFound, "no route with that id"));
                }
                Ok(Some(json!({ "removed": removed })))
            }
            Request::ListRoutes(_) => Ok(Some(json!({ "routes": self.backend.routes().await? }))),
            Request::GetRequests(_) => {
                let requests = self.backend.requests().await?;
                Ok(Some(json!({ "count": requests.len(), "requests": requests })))
            }
            Request::NewTab(a) => {
                let url = a.url.as_deref().map(|u| p.check_navigation(u)).transpose()?;
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                self.backend.new_tab().await?;
                self.last_snapshot = None;
                if let Some(url) = url {
                    self.backend.navigate(url.as_str(), a.wait_until, Duration::from_millis(timeout)).await?;
                }
                let tabs = self.backend.tabs().await?;
                Ok(Some(json!({ "tabs": tabs })))
            }
            Request::ListTabs(_) => {
                let tabs = self.backend.tabs().await?;
                Ok(Some(json!({ "tabs": tabs })))
            }
            Request::SwitchTab(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                if a.index.is_none() && a.url_contains.is_none() && a.title_contains.is_none() && !a.last {
                    return Err(CoreError::invalid("switch_tab needs index, url_contains, title_contains or last"));
                }
                let mut poll = Poll::new(timeout);
                let tab = loop {
                    let tabs = self.backend.tabs().await?;
                    let mut matching = tabs.iter().filter(|t| {
                        a.index.is_none_or(|i| t.index == i)
                            && a.url_contains.as_deref().is_none_or(|u| t.url.contains(u))
                            && a.title_contains.as_deref().is_none_or(|x| t.title.contains(x))
                    });
                    let found = if a.last { matching.next_back() } else { matching.next() };
                    if let Some(t) = found {
                        break t.clone();
                    }
                    if !poll.next().await {
                        return Err(CoreError::new(ErrorKind::NotFound, "no matching tab opened")
                            .with_details(json!({ "tabs": tabs })));
                    }
                };
                // Popups can lead anywhere: the navigation policy applies to them too.
                if !tab.url.is_empty() && tab.url != "about:blank" {
                    p.check_navigation(&tab.url).map_err(|e| {
                        CoreError::new(ErrorKind::PermissionDenied, format!("tab is outside the policy: {}", e.message))
                    })?;
                }
                self.backend.switch_tab(tab.index).await?;
                self.last_snapshot = None;
                let page = self.backend.page_state().await?;
                Ok(Some(json!({ "index": tab.index, "url": page.url, "title": page.title })))
            }
            Request::CloseTab(a) => {
                let tabs = self.backend.tabs().await?;
                let index = match a.index {
                    Some(i) => i,
                    None => tabs.iter().find(|t| t.active).map(|t| t.index).unwrap_or(0),
                };
                if tabs.len() <= 1 {
                    return Err(CoreError::invalid("cannot close the last tab; close the session instead"));
                }
                self.backend.close_tab(index).await?;
                self.last_snapshot = None;
                let tabs = self.backend.tabs().await?;
                Ok(Some(json!({ "tabs": tabs })))
            }
            Request::ListFrames(_) => {
                let frames = self.backend.frames().await?;
                Ok(Some(json!({ "frames": frames })))
            }
            Request::SwitchFrame(a) => {
                let timeout = p.timeout(a.timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                let chosen = [a.target.is_some(), a.name.is_some(), a.url_contains.is_some(), a.main];
                if chosen.iter().filter(|c| **c).count() != 1 {
                    return Err(CoreError::invalid(
                        "switch_frame needs exactly one of target, name, url_contains or main",
                    ));
                }
                let frame = if a.main {
                    None
                } else if let Some(t) = &a.target {
                    t.locator.validate()?;
                    let el = loop {
                        match self.resolve(t).await {
                            Ok(r) => break r.element,
                            Err(e) if e.recoverable && poll.next().await => continue,
                            Err(e) => return Err(e),
                        }
                    };
                    if el.tag != "iframe" && el.tag != "frame" {
                        return Err(CoreError::invalid(format!("{} is a <{}>, not an <iframe>", el.ref_id, el.tag)));
                    }
                    Some(FrameRef::Element(el.ref_id))
                } else {
                    let id = loop {
                        let frames = self.backend.frames().await?;
                        let hit = frames.iter().skip(1).find(|f| {
                            a.name.as_deref().is_none_or(|n| f.name == n)
                                && a.url_contains.as_deref().is_none_or(|u| f.url.contains(u))
                        });
                        if let Some(f) = hit {
                            break f.id.clone();
                        }
                        if !poll.next().await {
                            return Err(CoreError::new(ErrorKind::NotFound, "no matching frame")
                                .with_details(json!({ "frames": frames })));
                        }
                    };
                    Some(FrameRef::Id(id))
                };
                // A frame can still be loading right after its element appears.
                let info = loop {
                    match self.backend.enter_frame(frame.clone()).await {
                        Ok(info) => break info,
                        Err(e) if e.recoverable && poll.next().await => continue,
                        Err(e) => return Err(e),
                    }
                };
                self.last_snapshot = None;
                Ok(Some(serde_json::to_value(info).unwrap_or_default()))
            }
            Request::GetScreenshot(a) => {
                let stem = a.name.clone().unwrap_or_else(|| action_id.to_string());
                let ev = self.capture(&stem, action_id, a.full_page, a.format).await?;
                let data = json!({ "path": ev.path });
                ctx.evidence.push(ev);
                Ok(Some(data))
            }
            Request::AssertPageState(a) => {
                if a.conditions.is_empty() {
                    return Err(CoreError::invalid("assert_page_state needs at least one condition"));
                }
                let timeout = a.timeout_ms.unwrap_or(0).min(p.max_timeout_ms);
                ctx.timeout_ms = Some(timeout);
                let mut poll = Poll::new(timeout);
                let (passed, checks) = self.wait_all(&a.conditions, &mut poll).await?;
                self.assertions.push(AssertionRecord {
                    action_id: action_id.to_string(),
                    name: a.name.clone(),
                    passed,
                    checks: checks.clone(),
                });
                ctx.verification = Some(Verification {
                    status: if passed { VerificationStatus::Passed } else { VerificationStatus::Failed },
                    checks,
                    note: None,
                });
                if !passed {
                    return Err(CoreError::new(ErrorKind::VerificationFailed, "assertion failed"));
                }
                Ok(None)
            }
            Request::GetConsoleErrors(_) => {
                let errors = self.backend.console_errors().await?;
                Ok(Some(json!({ "count": errors.len(), "errors": errors })))
            }
            Request::GetNetworkFailures(_) => {
                let failures = self.backend.network_failures().await?;
                Ok(Some(json!({ "count": failures.len(), "failures": failures })))
            }
            Request::GetTestReport(_) => Ok(Some(self.report().await)),
            Request::GetCapabilities(_) => Ok(Some(serde_json::to_value(&self.caps).unwrap_or_default())),
        }
    }

    /// Resolve the target and perform one input action on it, re-resolving and
    /// retrying only for failures the backend guarantees happened before input
    /// was dispatched. Returns the ref that was acted on.
    async fn act(&mut self, target: &Target, act: Act<'_>, poll: &mut Poll, ctx: &mut Ctx) -> Result<String> {
        target.locator.validate()?;
        let mut retries: Vec<String> = Vec::new();
        let result = loop {
            let attempt: Result<String> = async {
                let resolved = self.resolve(target).await?;
                let ref_id = resolved.element.ref_id.clone();
                if let Act::Click { approved, consequential } = &act {
                    let reason = match consequential {
                        Some(true) => Some("caller marked the action as consequential".to_string()),
                        Some(false) => None,
                        None => self.policy.consequential_reason(&resolved.element),
                    };
                    if let Some(reason) = reason.filter(|_| self.policy.require_approval_for_consequential && !approved)
                    {
                        return Err(CoreError::new(
                            ErrorKind::ApprovalRequired,
                            "this click looks consequential; ask a human and repeat with approved=true",
                        )
                        .with_details(json!({ "reason": reason, "element": summarize(&resolved.element) })));
                    }
                }
                ctx.target = Some(resolved);
                match &act {
                    Act::Click { .. } => self.backend.click(&ref_id).await,
                    Act::Fill(v) => self.backend.fill(&ref_id, v).await,
                    Act::Focus => self.backend.focus(&ref_id).await,
                    Act::Hover => self.backend.hover(&ref_id).await,
                    Act::Select(o) => self.backend.select_option(&ref_id, o).await.map(|s| ctx.selected = Some(s)),
                }?;
                Ok(ref_id)
            }
            .await;
            match attempt {
                Err(e) if e.recoverable && !matches!(e.kind, ErrorKind::Timeout) => {
                    let stale_ref = e.kind == ErrorKind::StaleElement && matches!(target.locator, Locator::Ref { .. });
                    if stale_ref || !poll.next().await {
                        break Err(e);
                    }
                    if retries.last() != Some(&e.message) {
                        retries.push(e.message.clone());
                    }
                }
                other => break other,
            }
        };
        if !retries.is_empty() {
            ctx.notes.push(format!(
                "waited for the target before acting ({} poll attempts): {}",
                poll.attempt,
                retries.join("; ")
            ));
        }
        result.map_err(|e| {
            if e.recoverable && poll.remaining_ms() == 0 {
                let details = json!({ "last_error": e });
                CoreError::new(ErrorKind::Timeout, format!("gave up waiting for the target: {}", e.message))
                    .with_details(details)
            } else {
                e
            }
        })
    }

    async fn verify_expect(
        &mut self,
        expect: Option<&Condition>,
        poll: &mut Poll,
        ctx: &mut Ctx,
        what: &str,
    ) -> Result<()> {
        let Some(cond) = expect else {
            ctx.verification = Some(not_checked(&format!(
                "{what} was dispatched; no expected outcome was given, so success is not verified"
            )));
            return Ok(());
        };
        let (passed, checks) = self.wait_all(std::slice::from_ref(cond), poll).await?;
        ctx.verification = Some(Verification {
            status: if passed { VerificationStatus::Passed } else { VerificationStatus::Failed },
            checks,
            note: None,
        });
        if !passed {
            return Err(CoreError::new(
                ErrorKind::VerificationFailed,
                format!("{what} was dispatched but the expected outcome did not occur; do not repeat it blindly"),
            ));
        }
        Ok(())
    }

    async fn wait_all(&mut self, conds: &[Condition], poll: &mut Poll) -> Result<(bool, Vec<Check>)> {
        loop {
            let mut checks = Vec::with_capacity(conds.len());
            for c in conds {
                let (passed, observed) = self.check(c).await?;
                checks.push(Check { condition: serde_json::to_value(c).unwrap_or_default(), passed, observed });
            }
            if checks.iter().all(|c| c.passed) {
                return Ok((true, checks));
            }
            if !poll.next().await {
                return Ok((false, checks));
            }
        }
    }

    async fn check(&mut self, c: &Condition) -> Result<(bool, Value)> {
        Ok(match c {
            Condition::UrlEquals { value } => {
                let url = self.backend.page_state().await?.url;
                (url == *value, json!({ "url": url }))
            }
            Condition::UrlContains { value } => {
                let url = self.backend.page_state().await?.url;
                (url.contains(value.as_str()), json!({ "url": url }))
            }
            Condition::UrlMatches { pattern } => {
                let re = regex::Regex::new(pattern).map_err(|e| CoreError::invalid(format!("bad pattern: {e}")))?;
                let url = self.backend.page_state().await?.url;
                (re.is_match(&url), json!({ "url": url }))
            }
            Condition::TitleContains { value } => {
                let title = self.backend.page_state().await?.title;
                (title.contains(value.as_str()), json!({ "title": title }))
            }
            Condition::TitleEquals { value } => {
                let title = self.backend.page_state().await?.title;
                (title == *value, json!({ "title": title }))
            }
            Condition::TextEquals { target, text } | Condition::TextContains { target, text } => {
                match self.resolve_soft(target).await? {
                    Some(el) => {
                        let actual = match self.backend.read_text(&el.ref_id).await {
                            Ok(t) => t,
                            Err(e) if e.kind == ErrorKind::StaleElement => {
                                return Ok((false, json!({ "found": false })))
                            }
                            Err(e) => return Err(e),
                        };
                        let passed = match c {
                            Condition::TextEquals { .. } => actual == *text,
                            _ => actual.contains(text.as_str()),
                        };
                        (passed, json!({ "text": actual }))
                    }
                    None => (false, json!({ "found": false })),
                }
            }
            Condition::Visible { target } | Condition::Hidden { target } => {
                let matches = self.matches_for_condition(target).await?;
                let visible = match target.nth {
                    Some(n) => matches.get(n).is_some_and(|e| e.visible),
                    None => matches.iter().any(|e| e.visible),
                };
                let want = matches!(c, Condition::Visible { .. });
                (visible == want, json!({ "matches": matches.len(), "visible": visible }))
            }
            Condition::Count { target, count } => {
                let n = self.matches_for_condition(target).await?.len();
                (n == *count, json!({ "count": n }))
            }
            Condition::TextVisible { text } | Condition::TextAbsent { text } => {
                let loc = Locator::Text { text: text.clone(), exact: false };
                let visible = self.backend.query(&loc).await?.iter().any(|e| e.visible);
                let want = matches!(c, Condition::TextVisible { .. });
                (visible == want, json!({ "visible": visible }))
            }
            Condition::ValueEquals { target, value } => match self.resolve_soft(target).await? {
                Some(el) => {
                    let actual = self.backend.read_value(&el.ref_id).await?;
                    let masked = el.input_type.as_deref() == Some("password");
                    let observed = if masked { json!("<redacted>") } else { json!(actual) };
                    (actual.as_deref() == Some(value.as_str()), json!({ "value": observed }))
                }
                None => (false, json!({ "found": false })),
            },
            Condition::Enabled { target } => match self.resolve_soft(target).await? {
                Some(el) => (el.enabled, json!({ "enabled": el.enabled })),
                None => (false, json!({ "found": false })),
            },
            Condition::Checked { target, checked } => match self.resolve_soft(target).await? {
                Some(el) => (el.checked == Some(*checked), json!({ "checked": el.checked })),
                None => (false, json!({ "found": false })),
            },
            Condition::NoConsoleErrors {} => {
                let errors = self.backend.console_errors().await?;
                (errors.is_empty(), json!({ "count": errors.len(), "first": errors.first() }))
            }
            Condition::RequestSeen { url_contains, method } => {
                let requests = self.backend.requests().await?;
                let hits = requests
                    .iter()
                    .filter(|r| {
                        r.url.contains(url_contains.as_str())
                            && method.as_deref().is_none_or(|m| r.method.eq_ignore_ascii_case(m))
                    })
                    .count();
                (hits > 0, json!({ "matching_requests": hits, "total_requests": requests.len() }))
            }
            Condition::NoNetworkFailures {} => {
                let failures = self.backend.network_failures().await?;
                (failures.is_empty(), json!({ "count": failures.len(), "first": failures.first() }))
            }
        })
    }

    async fn matches_for_condition(&mut self, target: &Target) -> Result<Vec<ElementInfo>> {
        target.locator.validate()?;
        match self.query_all(&target.locator).await {
            Ok((m, _)) => Ok(m),
            Err(e) if e.kind == ErrorKind::StaleElement => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Resolve for a condition: "not found" is an observation, not an error.
    async fn resolve_soft(&mut self, target: &Target) -> Result<Option<ElementInfo>> {
        match self.resolve(target).await {
            Ok(r) => Ok(Some(r.element)),
            Err(e) if matches!(e.kind, ErrorKind::NotFound | ErrorKind::StaleElement) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn query_all(&mut self, locator: &Locator) -> Result<(Vec<ElementInfo>, String)> {
        match locator {
            Locator::Auto { query } => {
                const LABELS: [&str; 5] = ["accessible_name", "label", "test_id", "exact_text", "partial_text"];
                for (loc, label) in Locator::auto_chain(query).iter().zip(LABELS) {
                    let m = self.backend.query(loc).await?;
                    if !m.is_empty() {
                        return Ok((m, format!("auto:{label}")));
                    }
                }
                Ok((Vec::new(), "auto".into()))
            }
            Locator::Ref { ref_id } => Ok((vec![self.backend.describe(ref_id).await?], "ref".into())),
            other => Ok((self.backend.query(other).await?, other.strategy().into())),
        }
    }

    async fn resolve(&mut self, target: &Target) -> Result<ResolvedTarget> {
        let (matches, strategy) = self.query_all(&target.locator).await?;
        select(matches, strategy, target.nth)
    }

    async fn capture(&mut self, stem: &str, action_id: &str, full_page: bool, format: ImageFormat) -> Result<Evidence> {
        let ext = if format == ImageFormat::Jpeg { "jpg" } else { "png" };
        let path = self.policy.evidence_path(stem, ext)?;
        std::fs::create_dir_all(&self.policy.evidence_dir)
            .map_err(|e| CoreError::backend(format!("cannot create evidence directory: {e}")))?;
        self.backend.screenshot(&path, full_page).await?;
        let ev = Evidence { kind: "screenshot".into(), path: path.display().to_string(), action_id: action_id.into() };
        self.evidence.push(ev.clone());
        Ok(ev)
    }

    async fn report(&mut self) -> Value {
        let passed = self.assertions.iter().filter(|a| a.passed).count();
        let failed = self.assertions.len() - passed;
        let failed_actions: Vec<&LogEntry> = self.log.iter().filter(|l| !l.ok).collect();
        let console = self.backend.console_errors().await.ok();
        let network = self.backend.network_failures().await.ok();
        let verdict = if failed > 0 || !failed_actions.is_empty() {
            "failed"
        } else if self.assertions.is_empty() {
            "no_assertions"
        } else {
            "passed"
        };
        json!({
            "verdict": verdict,
            "session_id": self.id,
            "created_at_ms": self.created_at_ms,
            "backend": self.caps.backend,
            "engine": self.caps.engine,
            "engine_version": self.caps.engine_version,
            "actions": self.log.len(),
            "failed_actions": failed_actions,
            "assertions": { "passed": passed, "failed": failed, "records": self.assertions },
            "console_errors": console.as_ref().map(|c| c.len()),
            "network_failures": network.as_ref().map(|n| n.len()),
            "evidence": self.evidence,
        })
    }
}

fn not_checked(note: &str) -> Verification {
    Verification { status: VerificationStatus::NotChecked, checks: Vec::new(), note: Some(note.to_string()) }
}

/// Apply the documented selection rule. See [`Target`].
fn select(matches: Vec<ElementInfo>, strategy: String, nth: Option<usize>) -> Result<ResolvedTarget> {
    let candidates = matches.len();
    if candidates == 0 {
        return Err(CoreError::new(ErrorKind::NotFound, format!("no element matched ({strategy})")));
    }
    let any_visible = matches.iter().any(|e| e.visible);
    let pool: Vec<ElementInfo> =
        if any_visible { matches.into_iter().filter(|e| e.visible).collect() } else { matches };
    let element = match nth {
        Some(n) => pool.get(n).cloned().ok_or_else(|| {
            CoreError::new(ErrorKind::NotFound, format!("nth={n} requested but only {} matched", pool.len()))
        })?,
        None if pool.len() > 1 => {
            let listed: Vec<Value> = pool.iter().take(10).map(summarize).collect();
            return Err(CoreError::new(
                ErrorKind::Ambiguous,
                format!("{} elements matched ({strategy}); refine the locator or pass nth", pool.len()),
            )
            .with_details(json!({ "candidates": listed, "total": pool.len() })));
        }
        None => pool.into_iter().next().expect("non-empty"),
    };
    Ok(ResolvedTarget { element, strategy, candidates })
}

/// Writes a file only this user can read (on Unix; Windows uses the directory's ACL).
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    std::io::Write::write_all(&mut opts.open(path)?, bytes)
}
