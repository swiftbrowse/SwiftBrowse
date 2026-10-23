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
pub struct WaitForArgs {
    pub condition: Condition,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotArgs {
    #[serde(default)]
    pub full_page: bool,
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
    Fill(FillArgs),
    TypeText(TypeTextArgs),
    PressKey(PressKeyArgs),
    SelectOption(SelectOptionArgs),
    WaitFor(WaitForArgs),
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
            Request::Fill(_) => "fill",
            Request::TypeText(_) => "type_text",
            Request::PressKey(_) => "press_key",
            Request::SelectOption(_) => "select_option",
            Request::WaitFor(_) => "wait_for",
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
        OpSpec { name: "fill", description: "Replace a field's value and verify the resulting value.", schema: schema_of::<FillArgs> },
        OpSpec { name: "type_text", description: "Type text key by key into the target or focused element.", schema: schema_of::<TypeTextArgs> },
        OpSpec { name: "press_key", description: "Press a key or chord, optionally on a target and with an expected outcome.", schema: schema_of::<PressKeyArgs> },
        OpSpec { name: "select_option", description: "Choose an option of a <select> by value or label and verify the selection.", schema: schema_of::<SelectOptionArgs> },
        OpSpec { name: "wait_for", description: "Wait until a condition holds or the timeout expires.", schema: schema_of::<WaitForArgs> },
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
