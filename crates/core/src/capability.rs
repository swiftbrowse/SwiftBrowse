use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Full,
    Partial,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Capability {
    pub name: String,
    pub support: Support,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What a backend actually is and can do. Clients should inspect this before
/// relying on an operation; the core also checks it before dispatching.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityReport {
    pub backend: String,
    pub engine: String,
    pub engine_version: Option<String>,
    pub headless: bool,
    pub capabilities: Vec<Capability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl CapabilityReport {
    pub fn support(&self, name: &str) -> Support {
        self.capabilities.iter().find(|c| c.name == name).map(|c| c.support).unwrap_or(Support::Unsupported)
    }
}

pub fn cap(name: &str, support: Support, note: Option<&str>) -> Capability {
    Capability { name: name.to_string(), support, note: note.map(str::to_string) }
}

/// Capability names used across backends. Operations map onto these.
pub mod names {
    pub const NAVIGATE: &str = "navigate";
    pub const SNAPSHOT: &str = "page_snapshot";
    pub const ACCESSIBILITY_TREE: &str = "accessibility_tree";
    pub const SEMANTIC_LOCATORS: &str = "semantic_locators";
    pub const CSS_XPATH: &str = "css_xpath_locators";
    pub const TRUSTED_INPUT: &str = "trusted_input";
    pub const SELECT_OPTION: &str = "select_option";
    pub const SCREENSHOT: &str = "screenshot";
    pub const CONSOLE: &str = "console_diagnostics";
    pub const NETWORK: &str = "network_diagnostics";
    pub const DIALOGS: &str = "dialogs";
    pub const FRAMES: &str = "frames";
    pub const SHADOW_DOM: &str = "shadow_dom";
    pub const DOWNLOADS: &str = "downloads";
    pub const UPLOADS: &str = "uploads";
    pub const VISUAL_GROUNDING: &str = "visual_grounding";
    pub const ISOLATED_SESSIONS: &str = "isolated_sessions";
}
