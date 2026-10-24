use std::path::PathBuf;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, ErrorKind, Result};
use crate::model::ElementInfo;

/// Session policy. Page content can never change it: it is set by whoever
/// starts the runtime (CLI flags, MCP server config, SDK options).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub allowed_schemes: Vec<String>,
    /// Empty means any host. `*.example.com` matches subdomains and the apex.
    pub allowed_hosts: Vec<String>,
    pub blocked_hosts: Vec<String>,
    pub require_approval_for_consequential: bool,
    /// Case-insensitive patterns matched against a click target's accessible name and text.
    pub consequential_patterns: Vec<String>,
    pub default_timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub max_snapshot_elements: usize,
    pub max_full_snapshot_elements: usize,
    pub max_ax_nodes: usize,
    /// The only directory the runtime writes to.
    pub evidence_dir: PathBuf,
    pub screenshot_on_failure: bool,
    /// Allow the `evaluate` operation (arbitrary page script). Off by default for agents.
    pub allow_evaluate: bool,
    pub max_log_entries: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            allowed_schemes: ["http", "https", "about", "data"].map(String::from).to_vec(),
            allowed_hosts: Vec::new(),
            blocked_hosts: Vec::new(),
            require_approval_for_consequential: true,
            consequential_patterns: vec![
                r"\b(buy|purchase|pay|checkout|check out|place order|confirm order|submit order|subscribe)\b".into(),
                r"\b(delete|remove|destroy|erase|deactivate|close account|cancel subscription)\b".into(),
                r"\b(transfer|send money|withdraw|wire)\b".into(),
            ],
            default_timeout_ms: 10_000,
            max_timeout_ms: 120_000,
            max_snapshot_elements: 150,
            max_full_snapshot_elements: 1_500,
            max_ax_nodes: 500,
            evidence_dir: std::env::temp_dir().join("swiftbrowse-evidence"),
            screenshot_on_failure: true,
            allow_evaluate: false,
            max_log_entries: 500,
        }
    }
}

impl Policy {
    pub fn timeout(&self, requested: Option<u64>) -> u64 {
        requested.unwrap_or(self.default_timeout_ms).min(self.max_timeout_ms)
    }

    pub fn check_navigation(&self, raw: &str) -> Result<url::Url> {
        let url = url::Url::parse(raw).map_err(|e| CoreError::invalid(format!("invalid URL {raw:?}: {e}")))?;
        let scheme = url.scheme();
        if !self.allowed_schemes.iter().any(|s| s == scheme) {
            return Err(CoreError::new(
                ErrorKind::PermissionDenied,
                format!("scheme {scheme:?} is not allowed by policy"),
            )
            .with_details(serde_json::json!({ "allowed_schemes": self.allowed_schemes })));
        }
        if let Some(host) = url.host_str() {
            let host = host.to_ascii_lowercase();
            if self.blocked_hosts.iter().any(|p| host_matches(&host, p)) {
                return Err(CoreError::new(ErrorKind::PermissionDenied, format!("host {host} is blocked by policy")));
            }
            if !self.allowed_hosts.is_empty() && !self.allowed_hosts.iter().any(|p| host_matches(&host, p)) {
                return Err(CoreError::new(
                    ErrorKind::PermissionDenied,
                    format!("host {host} is not in the allow list"),
                )
                .with_details(serde_json::json!({ "allowed_hosts": self.allowed_hosts })));
            }
        }
        Ok(url)
    }

    /// Returns the matched pattern when the element looks like it triggers a consequential action.
    pub fn consequential_reason(&self, el: &ElementInfo) -> Option<String> {
        let haystack = format!("{} {}", el.name, el.text);
        self.consequential_patterns.iter().find_map(|p| {
            let re = Regex::new(&format!("(?i){p}")).ok()?;
            re.find(&haystack).map(|m| format!("target text matches {:?}", m.as_str()))
        })
    }

    /// Validates a caller-supplied file name stem so writes stay inside `evidence_dir`.
    pub fn evidence_path(&self, stem: &str, ext: &str) -> Result<PathBuf> {
        let ok = !stem.is_empty()
            && stem.len() <= 100
            && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            && !stem.starts_with('.');
        if !ok {
            return Err(CoreError::invalid("evidence name may only contain letters, digits, '-', '_' and '.'"));
        }
        Ok(self.evidence_dir.join(format!("{stem}.{ext}")))
    }
}

fn host_matches(host: &str, pattern: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == pattern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str) -> ElementInfo {
        ElementInfo {
            ref_id: "e1".into(),
            tag: "button".into(),
            role: Some("button".into()),
            name: name.into(),
            text: name.into(),
            value: None,
            test_id: None,
            input_type: None,
            href: None,
            checked: None,
            visible: true,
            enabled: true,
            editable: false,
        }
    }

    #[test]
    fn navigation_policy() {
        let mut p = Policy::default();
        assert!(p.check_navigation("https://example.com/a").is_ok());
        assert_eq!(p.check_navigation("file:///etc/passwd").unwrap_err().kind, ErrorKind::PermissionDenied);
        assert_eq!(p.check_navigation("javascript:alert(1)").unwrap_err().kind, ErrorKind::PermissionDenied);
        p.allowed_hosts = vec!["*.example.com".into()];
        assert!(p.check_navigation("https://shop.example.com").is_ok());
        assert!(p.check_navigation("https://example.com").is_ok());
        assert!(p.check_navigation("https://evil-example.com").is_err());
        p.blocked_hosts = vec!["shop.example.com".into()];
        assert!(p.check_navigation("https://shop.example.com").is_err());
    }

    #[test]
    fn consequential_detection() {
        let p = Policy::default();
        assert!(p.consequential_reason(&el("Place order")).is_some());
        assert!(p.consequential_reason(&el("Delete account")).is_some());
        assert!(p.consequential_reason(&el("Show details")).is_none());
        assert!(p.consequential_reason(&el("Payment history")).is_none());
    }

    #[test]
    fn evidence_names_cannot_escape() {
        let p = Policy::default();
        assert!(p.evidence_path("step-1", "png").is_ok());
        assert!(p.evidence_path("../x", "png").is_err());
        assert!(p.evidence_path("a/b", "png").is_err());
        assert!(p.evidence_path("", "png").is_err());
    }
}
