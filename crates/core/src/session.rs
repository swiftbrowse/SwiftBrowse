//! The execution engine: inspect → resolve → check policy → act → wait → verify → report.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::backend::Backend;
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
            match self.capture(&format!("{action_id}-failure"), &action_id, false).await {
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
                Ok(Some(serde_json::to_value(nav).unwrap_or_default()))
            }
            Request::GetPageSnapshot(a) => {
                let cap = match a.mode {
                    SnapshotMode::Compact => p.max_snapshot_elements,
                    SnapshotMode::Full => p.max_full_snapshot_elements,
                };
                let max = a.max_elements.unwrap_or(cap).min(cap);
                let snap = self.backend.snapshot(a.mode, max).await?;
                if snap.truncated {
                    ctx.notes.push(format!(
                        "snapshot truncated to {} of {} elements; use find_element or a full snapshot for more",
                        snap.elements.len(),
                        snap.total_elements
                    ));
                }
                let data = match (&self.last_snapshot, a.since_last) {
                    (Some(prev), true) => json!({
                        "url": snap.url,
                        "title": snap.title,
                        "diff": snap.diff_from(prev),
                        "messages": snap.messages,
                    }),
                    _ => {
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
                self.verify_expect(a.expect.as_ref(), &mut poll, ctx, "click").await?;
                Ok(None)
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
            Request::GetScreenshot(a) => {
                let stem = a.name.clone().unwrap_or_else(|| action_id.to_string());
                let ev = self.capture(&stem, action_id, a.full_page).await?;
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

    async fn capture(&mut self, stem: &str, action_id: &str, full_page: bool) -> Result<Evidence> {
        let path = self.policy.evidence_path(stem, "png")?;
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
