//! Page-level data shapes returned by backends.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One element as seen by the automation layer. `ref_id` is a session-scoped
/// handle that stays valid until the element is detached or the document changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ElementInfo {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Omitted when true.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub visible: bool,
    /// Omitted when true.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Omitted when false.
    #[serde(default, skip_serializing_if = "is_false")]
    pub editable: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

impl ElementInfo {
    /// One-line rendering used by the text snapshot format.
    pub fn to_line(&self) -> String {
        use std::fmt::Write;
        let mut s = self.role.clone().unwrap_or_else(|| self.tag.clone());
        if !self.name.is_empty() {
            let _ = write!(s, " {:?}", clip(&self.name, 80));
        }
        let _ = write!(s, " [{}]", self.ref_id);
        if let Some(t) =
            self.input_type.as_deref().filter(|t| !matches!(*t, "text" | "checkbox" | "radio" | "submit" | "button"))
        {
            let _ = write!(s, " type={t}");
        }
        let toggle = matches!(self.input_type.as_deref(), Some("checkbox" | "radio"));
        if let Some(v) = self.value.as_deref().filter(|v| !v.is_empty() && !toggle) {
            let _ = write!(s, " value={:?}", clip(v, 60));
        }
        match self.checked {
            Some(true) => s.push_str(" checked"),
            Some(false) => s.push_str(" unchecked"),
            None => {}
        }
        if !self.enabled {
            s.push_str(" disabled");
        }
        if !self.visible {
            s.push_str(" hidden");
        }
        if let Some(id) = &self.test_id {
            let _ = write!(s, " testid={id:?}");
        }
        if let Some(h) = &self.href {
            let _ = write!(s, " -> {}", clip(h, 80));
        }
        if !self.text.is_empty() && self.text != self.name {
            let _ = write!(s, ": {}", clip(&self.text, 60));
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageState {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Heading {
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ref")]
    pub ref_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PageSnapshot {
    pub url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headings: Vec<Heading>,
    /// Visible interactive elements (or all elements in full mode), bounded by `max_elements`.
    pub elements: Vec<ElementInfo>,
    /// Open dialogs, alerts, live regions and validation messages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
    pub total_elements: usize,
    pub truncated: bool,
    /// Set when the page was still rendering and the snapshot waited for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_after_load_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub still_empty: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub still_changing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotDiff {
    pub url_changed: bool,
    pub title_changed: bool,
    pub added: Vec<ElementInfo>,
    pub removed: Vec<String>,
    pub changed: Vec<ElementInfo>,
    pub messages_changed: bool,
}

impl SnapshotDiff {
    /// Text rendering: `+` added, `~` changed, `-` removed refs; messages are repeated in full.
    pub fn to_text(&self, now: &PageSnapshot) -> String {
        let mut out = String::new();
        if self.url_changed || self.title_changed {
            out.push_str(&now.header());
        }
        for e in &self.added {
            out.push_str(&format!("+ {}\n", e.to_line()));
        }
        for e in &self.changed {
            out.push_str(&format!("~ {}\n", e.to_line()));
        }
        if !self.removed.is_empty() {
            out.push_str(&format!("- {}\n", self.removed.join(" ")));
        }
        now.messages_text(&mut out);
        if out.is_empty() {
            out.push_str("no changes\n");
        }
        out
    }
}

impl PageSnapshot {
    fn header(&self) -> String {
        format!("page: {} | {}\n", clip(&self.title, 100), clip(&self.url, 200))
    }

    fn messages_text(&self, out: &mut String) {
        for m in &self.messages {
            out.push_str(&format!("! {}: {}", m.kind, clip(&m.text, 200)));
            if let Some(r) = &m.ref_id {
                out.push_str(&format!(" [{r}]"));
            }
            out.push('\n');
        }
    }

    /// Compact text rendering: header, headings, one line per element, messages.
    pub fn to_text(&self) -> String {
        let mut out = self.header();
        if !self.headings.is_empty() {
            let hs: Vec<String> = self.headings.iter().map(|h| format!("h{} {}", h.level, clip(&h.text, 80))).collect();
            out.push_str(&format!("headings: {}\n", hs.join("; ")));
        }
        for e in &self.elements {
            out.push_str(&e.to_line());
            out.push('\n');
        }
        self.messages_text(&mut out);
        if self.truncated {
            out.push_str(&format!(
                "… {} of {} elements shown; use find_element for others\n",
                self.elements.len(),
                self.total_elements
            ));
        }
        out
    }

    pub fn diff_from(&self, prev: &PageSnapshot) -> SnapshotDiff {
        use std::collections::HashMap;
        let old: HashMap<&str, &ElementInfo> = prev.elements.iter().map(|e| (e.ref_id.as_str(), e)).collect();
        let new: HashMap<&str, &ElementInfo> = self.elements.iter().map(|e| (e.ref_id.as_str(), e)).collect();
        SnapshotDiff {
            url_changed: self.url != prev.url,
            title_changed: self.title != prev.title,
            added: self.elements.iter().filter(|e| !old.contains_key(e.ref_id.as_str())).cloned().collect(),
            removed: prev
                .elements
                .iter()
                .filter(|e| !new.contains_key(e.ref_id.as_str()))
                .map(|e| e.ref_id.clone())
                .collect(),
            changed: self
                .elements
                .iter()
                .filter(|e| old.get(e.ref_id.as_str()).is_some_and(|o| *o != *e))
                .cloned()
                .collect(),
            messages_changed: self.messages != prev.messages,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationInfo {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub same_document: bool,
    /// Set when the page was still an empty shell at `load` and SwiftBrowse
    /// waited for client-side rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_after_load_ms: Option<u64>,
    /// The page still looked empty when the render wait ended.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub still_empty: bool,
    /// Visible content was still being added or removed when the wait ended.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub still_changing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NetworkFailure {
    pub url: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub resource_type: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DialogEvent {
    pub kind: String,
    pub message: String,
    /// What the backend did with it: "accepted" or "dismissed".
    pub handled: String,
    pub timestamp_ms: u64,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
