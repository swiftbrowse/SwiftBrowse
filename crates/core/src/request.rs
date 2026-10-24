//! The single automation contract. MCP tool names, CLI script `op` values and
//! SDK method names all map one-to-one onto [`Request`] variants.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::locator::Target;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitUntil {
    DomContentLoaded,
    #[default]
    Load,
    NetworkIdle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    /// Visible interactive elements, headings and messages.
    #[default]
    Compact,
    /// Every element with a role, name or text, still bounded by `max_elements`.
    Full,
}

/// A checkable page condition, used by `wait_for`, `assert_page_state` and `expect`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    UrlEquals {
        value: String,
    },
    UrlContains {
        value: String,
    },
    /// Regular expression matched against the full URL.
    UrlMatches {
        pattern: String,
    },
    TitleContains {
        value: String,
    },
    TitleEquals {
        value: String,
    },
    /// At least one visible match (or the `nth` match is visible).
    Visible {
        target: Target,
    },
    /// No visible match.
    Hidden {
        target: Target,
    },
    /// Exact number of matches, visible or not.
    Count {
        target: Target,
        count: usize,
    },
    TextVisible {
        text: String,
    },
    TextAbsent {
        text: String,
    },
    /// The element's whole visible text, whitespace-normalized, equals `text`.
    TextEquals {
        target: Target,
        text: String,
    },
    /// The element's visible text contains `text`.
    TextContains {
        target: Target,
        text: String,
    },
    ValueEquals {
        target: Target,
        value: String,
    },
    Enabled {
        target: Target,
    },
    Checked {
        target: Target,
        checked: bool,
    },
    NoConsoleErrors {},
    NoNetworkFailures {},
    /// The page has sent a request whose URL contains `url_contains` (and with this method).
    RequestSeen {
        url_contains: String,
        #[serde(default)]
        method: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigateArgs {
    pub url: String,
    #[serde(default)]
    pub wait_until: WaitUntil,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotFormat {
    /// One line per element, e.g. `textbox "Email" [e3] type=email`. Smallest for agent context.
    #[default]
    Text,
    /// Structured objects, for programmatic consumers.
    Json,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotArgs {
    #[serde(default)]
    pub mode: SnapshotMode,
    #[serde(default)]
    pub format: SnapshotFormat,
    /// Return only what changed since this session's previous snapshot.
    #[serde(default)]
    pub since_last: bool,
    #[serde(default)]
    pub max_elements: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccessibilityTreeArgs {
    #[serde(default)]
    pub max_nodes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindArgs {
    pub target: Target,
    /// Return every match instead of requiring a unique one.
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClickArgs {
    pub target: Target,
    /// Outcome to verify after the click. Without it the result reports the
    /// click as dispatched but unverified.
    #[serde(default)]
    pub expect: Option<Condition>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Set after a human approved a consequential action.
    #[serde(default)]
    pub approved: bool,
    /// Override the policy's consequential-action detection.
    #[serde(default)]
    pub consequential: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FillArgs {
    pub target: Target,
    pub value: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeTextArgs {
    /// Element to focus first; omit to type into the focused element.
    #[serde(default)]
    pub target: Option<Target>,
    pub text: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PressKeyArgs {
    /// Key name such as `Enter`, `Tab`, `Escape`, `ArrowDown`, `a`, or a chord like `Control+a`.
    pub key: String,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub expect: Option<Condition>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectOptionArgs {
    pub target: Target,
    /// Option value or visible label.
    pub option: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HoverArgs {
    pub target: Target,
    #[serde(default)]
    pub expect: Option<Condition>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckArgs {
    /// A checkbox, radio button, or element with `aria-checked`.
    pub target: Target,
    /// The state to leave it in. Default true; false unchecks.
    #[serde(default = "yes")]
    pub checked: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryArgs {
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UploadFileArgs {
    /// An `<input type="file">` (it may be hidden behind a styled button).
    pub target: Target,
    /// Files to select; an empty list clears the selection. Must lie in a
    /// directory the policy allows for uploads.
    pub files: Vec<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RouteAction {
    /// Answer with `status`, `body` / `json` and `headers` instead of the network.
    #[default]
    Fulfill,
    /// Fail the request like a network error (`error`, default "Failed").
    Abort,
    /// Let it through unchanged (to carve an exception out of a broader route).
    Continue,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteArgs {
    /// URL glob: `**` matches anything, `*` anything but `/`, `?` one character,
    /// e.g. `**/api/todos*`.
    #[serde(default)]
    pub url: Option<String>,
    /// Regular expression matched against the full URL, instead of `url`.
    #[serde(default)]
    pub url_regex: Option<String>,
    /// Only this HTTP method (e.g. `POST`).
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub action: RouteAction,
    /// Response status for `fulfill` (default 200).
    #[serde(default)]
    pub status: Option<u16>,
    /// Response body for `fulfill`.
    #[serde(default)]
    pub body: Option<String>,
    /// JSON response body for `fulfill`; sets `content-type: application/json`.
    #[serde(default)]
    pub json: Option<serde_json::Value>,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    /// Network error for `abort`: Failed, Aborted, TimedOut, AccessDenied,
    /// ConnectionClosed, ConnectionReset, ConnectionRefused, NameNotResolved,
    /// InternetDisconnected, BlockedByClient.
    #[serde(default)]
    pub error: Option<String>,
    /// Wait this long before answering (to test loading states).
    #[serde(default)]
    pub delay_ms: Option<u64>,
    /// Only the first N matching requests; afterwards the route is removed.
    #[serde(default)]
    pub times: Option<u32>,
}

impl RouteArgs {
    /// The URL matcher, from `url` (glob) or `url_regex`.
    pub fn matcher(&self) -> crate::Result<regex::Regex> {
        let pattern = match (&self.url, &self.url_regex) {
            (Some(glob), None) => glob_to_regex(glob),
            (None, Some(re)) => re.clone(),
            _ => return Err(crate::CoreError::invalid("route needs exactly one of url (glob) or url_regex")),
        };
        regex::Regex::new(&pattern).map_err(|e| crate::CoreError::invalid(format!("bad route pattern: {e}")))
    }
}

/// Playwright-style URL glob to an anchored regular expression.
pub fn glob_to_regex(glob: &str) -> String {
    let mut out = String::from("^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                out.push_str(".*");
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push('.'),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out.push('$');
    out
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DomSnapshotArgs {
    /// Mark this element (a ref) with `data-sb-target` in the snapshot.
    #[serde(default)]
    pub mark: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StopVideoArgs {
    /// File name stem in the evidence directory (default `video`).
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageStateArgs {
    /// Name of the saved state (letters, digits, '-', '_', '.'); stored as
    /// `<name>.json` in the state directory, in Playwright's storageState format.
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnrouteArgs {
    /// Route id from `route`; all routes when omitted.
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewTabArgs {
    /// Page to open in the new tab; blank when omitted.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub wait_until: WaitUntil,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwitchTabArgs {
    /// Tab index from `list_tabs`.
    #[serde(default)]
    pub index: Option<usize>,
    /// First tab whose URL contains this text. Waits for it to open (e.g. a popup).
    #[serde(default)]
    pub url_contains: Option<String>,
    /// First tab whose title contains this text. Waits for it to open.
    #[serde(default)]
    pub title_contains: Option<String>,
    /// The newest tab.
    #[serde(default)]
    pub last: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseTabArgs {
    /// Tab index from `list_tabs`; the active tab when omitted.
    #[serde(default)]
    pub index: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwitchFrameArgs {
    /// The `<iframe>` element, in the current frame.
    #[serde(default)]
    pub target: Option<Target>,
    /// Frame name (`<iframe name>`), anywhere in the tab.
    #[serde(default)]
    pub name: Option<String>,
    /// First frame whose URL contains this text, anywhere in the tab.
    #[serde(default)]
    pub url_contains: Option<String>,
    /// Back to the tab's main frame.
    #[serde(default)]
    pub main: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateArgs {
    /// JavaScript expression evaluated in the page; promises are awaited and the
    /// JSON-serializable result is returned. Disabled unless the policy allows it.
    pub expression: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitForArgs {
    pub condition: Condition,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    #[default]
    Png,
    /// Smaller files (quality 70).
    Jpeg,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotArgs {
    #[serde(default)]
    pub full_page: bool,
    #[serde(default)]
    pub format: ImageFormat,
    /// File name stem inside the evidence directory; path separators are rejected.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssertArgs {
    pub conditions: Vec<Condition>,
    /// Wait up to this long for all conditions to hold. Default 0: check once.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Label recorded in the test report.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Navigate(NavigateArgs),
    GetPageSnapshot(SnapshotArgs),
    GetAccessibilityTree(AccessibilityTreeArgs),
    FindElement(FindArgs),
    Click(ClickArgs),
    Hover(HoverArgs),
    Check(CheckArgs),
    Fill(FillArgs),
    TypeText(TypeTextArgs),
    PressKey(PressKeyArgs),
    SelectOption(SelectOptionArgs),
    UploadFile(UploadFileArgs),
    WaitFor(WaitForArgs),
    GoBack(HistoryArgs),
    GoForward(HistoryArgs),
    Reload(HistoryArgs),
    Evaluate(EvaluateArgs),
    GetDomSnapshot(DomSnapshotArgs),
    StartVideo(NoArgs),
    StopVideo(StopVideoArgs),
    SaveStorageState(StorageStateArgs),
    LoadStorageState(StorageStateArgs),
    Route(RouteArgs),
    Unroute(UnrouteArgs),
    ListRoutes(NoArgs),
    GetRequests(NoArgs),
    NewTab(NewTabArgs),
    ListTabs(NoArgs),
    SwitchTab(SwitchTabArgs),
    CloseTab(CloseTabArgs),
    ListFrames(NoArgs),
    SwitchFrame(SwitchFrameArgs),
    GetScreenshot(ScreenshotArgs),
    AssertPageState(AssertArgs),
    GetConsoleErrors(NoArgs),
    GetNetworkFailures(NoArgs),
    GetTestReport(NoArgs),
    GetCapabilities(NoArgs),
}

impl Request {
    pub fn op(&self) -> &'static str {
        match self {
            Request::Navigate(_) => "navigate",
            Request::GetPageSnapshot(_) => "get_page_snapshot",
            Request::GetAccessibilityTree(_) => "get_accessibility_tree",
            Request::FindElement(_) => "find_element",
            Request::Click(_) => "click",
            Request::Hover(_) => "hover",
            Request::Check(_) => "check",
            Request::Fill(_) => "fill",
            Request::TypeText(_) => "type_text",
            Request::PressKey(_) => "press_key",
            Request::SelectOption(_) => "select_option",
            Request::UploadFile(_) => "upload_file",
            Request::WaitFor(_) => "wait_for",
            Request::GoBack(_) => "go_back",
            Request::GoForward(_) => "go_forward",
            Request::Reload(_) => "reload",
            Request::Evaluate(_) => "evaluate",
            Request::GetDomSnapshot(_) => "get_dom_snapshot",
            Request::StartVideo(_) => "start_video",
            Request::StopVideo(_) => "stop_video",
            Request::SaveStorageState(_) => "save_storage_state",
            Request::LoadStorageState(_) => "load_storage_state",
            Request::Route(_) => "route",
            Request::Unroute(_) => "unroute",
            Request::ListRoutes(_) => "list_routes",
            Request::GetRequests(_) => "get_requests",
            Request::NewTab(_) => "new_tab",
            Request::ListTabs(_) => "list_tabs",
            Request::SwitchTab(_) => "switch_tab",
            Request::CloseTab(_) => "close_tab",
            Request::ListFrames(_) => "list_frames",
            Request::SwitchFrame(_) => "switch_frame",
            Request::GetScreenshot(_) => "get_screenshot",
            Request::AssertPageState(_) => "assert_page_state",
            Request::GetConsoleErrors(_) => "get_console_errors",
            Request::GetNetworkFailures(_) => "get_network_failures",
            Request::GetTestReport(_) => "get_test_report",
            Request::GetCapabilities(_) => "get_capabilities",
        }
    }

    /// Build a request from an operation name and its JSON arguments.
    pub fn from_op(op: &str, args: serde_json::Value) -> crate::Result<Request> {
        let mut obj = match args {
            serde_json::Value::Object(m) => m,
            serde_json::Value::Null => serde_json::Map::new(),
            _ => return Err(crate::CoreError::invalid("arguments must be a JSON object")),
        };
        obj.insert("op".into(), serde_json::Value::String(op.to_string()));
        serde_json::from_value(serde_json::Value::Object(obj))
            .map_err(|e| crate::CoreError::invalid(format!("invalid arguments for {op}: {e}")))
    }
}

/// Operation name, description and argument schema, used to publish MCP tools
/// and to generate SDK bindings from one source.
pub struct OpSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> serde_json::Value,
}

fn schema_of<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or_default()
}

pub fn op_specs() -> Vec<OpSpec> {
    vec![
        OpSpec { name: "navigate", description: "Open a URL, wait for the requested load state and report the HTTP status.", schema: schema_of::<NavigateArgs> },
        OpSpec { name: "get_page_snapshot", description: "Compact structured view of the page: URL, title, headings, visible interactive elements with ref handles, dialogs, alerts and validation messages. Use since_last to get only changes.", schema: schema_of::<SnapshotArgs> },
        OpSpec { name: "get_accessibility_tree", description: "Bounded accessibility tree (role, name, value) from the browser.", schema: schema_of::<AccessibilityTreeArgs> },
        OpSpec { name: "find_element", description: "Resolve a locator. Fails with `ambiguous` when several elements match and no nth is given.", schema: schema_of::<FindArgs> },
        OpSpec { name: "click", description: "Click an element after checking it is visible, enabled and not covered. Pass expect to verify the outcome. Consequential actions (buy, pay, delete, ...) need approved=true.", schema: schema_of::<ClickArgs> },
        OpSpec { name: "hover", description: "Move the mouse over an element after checking it is visible and not covered. Pass expect to verify what appears.", schema: schema_of::<HoverArgs> },
        OpSpec { name: "check", description: "Leave a checkbox, radio or aria-checked element checked (or unchecked with checked=false), clicking only if needed, and verify the state.", schema: schema_of::<CheckArgs> },
        OpSpec { name: "fill", description: "Replace a field's value and verify the resulting value.", schema: schema_of::<FillArgs> },
        OpSpec { name: "type_text", description: "Type text key by key into the target or focused element.", schema: schema_of::<TypeTextArgs> },
        OpSpec { name: "press_key", description: "Press a key or chord, optionally on a target and with an expected outcome.", schema: schema_of::<PressKeyArgs> },
        OpSpec { name: "select_option", description: "Choose an option of a <select> by value or label and verify the selection.", schema: schema_of::<SelectOptionArgs> },
        OpSpec { name: "upload_file", description: "Select files on an <input type=file> and verify the selection. Only files inside directories the policy allows.", schema: schema_of::<UploadFileArgs> },
        OpSpec { name: "wait_for", description: "Wait until a condition holds or the timeout expires.", schema: schema_of::<WaitForArgs> },
        OpSpec { name: "go_back", description: "Go back one history entry and wait for the page to load.", schema: schema_of::<HistoryArgs> },
        OpSpec { name: "go_forward", description: "Go forward one history entry and wait for the page to load.", schema: schema_of::<HistoryArgs> },
        OpSpec { name: "reload", description: "Reload the page and wait for it to load.", schema: schema_of::<HistoryArgs> },
        OpSpec { name: "evaluate", description: "Run a JavaScript expression in the page and return its JSON result. Only available when the session policy allows scripts (the test runner does; MCP needs --allow-evaluate).", schema: schema_of::<EvaluateArgs> },
        OpSpec { name: "get_dom_snapshot", description: "A self-contained, script-free HTML copy of the page (form state, shadow roots, live CSS) for trace viewers. Large; prefer get_page_snapshot to read a page.", schema: schema_of::<DomSnapshotArgs> },
        OpSpec { name: "start_video", description: "Start recording a video of the active tab (follows tab switches).", schema: schema_of::<NoArgs> },
        OpSpec { name: "stop_video", description: "Stop recording and save the video (WebM via ffmpeg) to the evidence directory.", schema: schema_of::<StopVideoArgs> },
        OpSpec { name: "save_storage_state", description: "Save this session's cookies and the localStorage of its open origins (a logged-in state) under a name, in Playwright's storageState format.", schema: schema_of::<StorageStateArgs> },
        OpSpec { name: "load_storage_state", description: "Restore a saved state (cookies and localStorage) into this session, e.g. to start logged in.", schema: schema_of::<StorageStateArgs> },
        OpSpec { name: "route", description: "Mock the network: requests matching a URL glob or regex (and method) are fulfilled with a status/body/JSON, aborted with a network error, or let through. Newest route wins; times limits uses.", schema: schema_of::<RouteArgs> },
        OpSpec { name: "unroute", description: "Remove a route by id, or all routes.", schema: schema_of::<UnrouteArgs> },
        OpSpec { name: "list_routes", description: "Active routes with how often each matched.", schema: schema_of::<NoArgs> },
        OpSpec { name: "get_requests", description: "Requests the page made (method, URL, status, whether mocked), newest last.", schema: schema_of::<NoArgs> },
        OpSpec { name: "new_tab", description: "Open a new tab (optionally at a URL) in this session and make it active.", schema: schema_of::<NewTabArgs> },
        OpSpec { name: "list_tabs", description: "Open tabs of this session, including popups and target=_blank pages, with the active one marked.", schema: schema_of::<NoArgs> },
        OpSpec { name: "switch_tab", description: "Make a tab active by index, URL, title, or the newest one; waits for a matching tab to open (e.g. a popup).", schema: schema_of::<SwitchTabArgs> },
        OpSpec { name: "close_tab", description: "Close a tab (the active one by default); the opener or the last tab becomes active.", schema: schema_of::<CloseTabArgs> },
        OpSpec { name: "list_frames", description: "Frames (iframes) of the active tab with their names and URLs.", schema: schema_of::<NoArgs> },
        OpSpec { name: "switch_frame", description: "Act inside an iframe (by its element, name or URL) until switch_frame with main=true or a navigation. Works for cross-site iframes.", schema: schema_of::<SwitchFrameArgs> },
        OpSpec { name: "get_screenshot", description: "Save a PNG screenshot to the evidence directory and return its path.", schema: schema_of::<ScreenshotArgs> },
        OpSpec { name: "assert_page_state", description: "Check URL, title, element, text, value or diagnostics conditions. Recorded in the test report.", schema: schema_of::<AssertArgs> },
        OpSpec { name: "get_console_errors", description: "Console errors and uncaught exceptions seen in this session.", schema: schema_of::<NoArgs> },
        OpSpec { name: "get_network_failures", description: "Failed requests and HTTP >= 400 responses seen in this session.", schema: schema_of::<NoArgs> },
        OpSpec { name: "get_test_report", description: "Assertions, failed actions, diagnostics and evidence for this session.", schema: schema_of::<NoArgs> },
        OpSpec { name: "get_capabilities", description: "Backend, engine version and per-feature support for this session.", schema: schema_of::<NoArgs> },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_op_validates_arguments() {
        let r = Request::from_op("click", serde_json::json!({"target": {"by": "auto", "query": "Save"}})).unwrap();
        assert_eq!(r.op(), "click");
        let err = Request::from_op("click", serde_json::json!({"target": {"by": "auto", "query": "x"}, "bogus": 1}))
            .unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::InvalidArgument);
        assert!(Request::from_op("teleport", serde_json::json!({})).is_err());
    }

    #[test]
    fn route_globs_follow_playwright() {
        let m = |glob: &str, url: &str| regex::Regex::new(&glob_to_regex(glob)).unwrap().is_match(url);
        assert!(m("**/api/todos", "http://x.test/app/api/todos"));
        assert!(!m("**/api/todos", "http://x.test/api/todos/1"));
        assert!(m("**/api/todos*", "http://x.test/api/todos?page=2"));
        assert!(m("http://x.test/*.png", "http://x.test/a.png"));
        assert!(!m("http://x.test/*.png", "http://x.test/img/a.png"), "* stops at /");
        assert!(m("**/v?/users", "http://x.test/v2/users"));
        assert!(m("**/a+b(c)", "http://x.test/a+b(c)"), "regex characters are literal");
        let both = RouteArgs { url: Some("x".into()), url_regex: Some("y".into()), ..Default::default() };
        assert!(both.matcher().is_err());
    }

    #[test]
    fn every_op_has_a_spec_and_parses_with_minimal_args() {
        for spec in op_specs() {
            let schema = (spec.schema)();
            assert!(schema.is_object(), "{} schema", spec.name);
            let required = schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
            if required.is_empty() {
                Request::from_op(spec.name, serde_json::json!({})).unwrap();
            }
        }
    }
}
