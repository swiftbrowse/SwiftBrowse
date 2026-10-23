use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How to find an element. Strategies are listed in the order the core prefers
/// them; `auto` walks that order and reports which strategy matched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "by", rename_all = "snake_case", deny_unknown_fields)]
pub enum Locator {
    /// A `ref` handle returned by a snapshot or a previous lookup.
    Ref {
        #[serde(rename = "ref")]
        ref_id: String,
    },
    /// Accessible role plus optional accessible name. `role: "*"` matches any non-generic role.
    Role {
        role: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        exact: bool,
    },
    /// A form control whose associated label (or aria-label) matches.
    Label {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    /// `data-testid`, `data-test-id` or `data-test` attribute.
    TestId {
        id: String,
    },
    /// Innermost elements whose text matches.
    Text {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    Css {
        selector: String,
    },
    Xpath {
        expr: String,
    },
    /// Try accessible name, then label, then test id, then exact text, then partial text.
    Auto {
        query: String,
    },
}

impl Locator {
    pub fn strategy(&self) -> &'static str {
        match self {
            Locator::Ref { .. } => "ref",
            Locator::Role { .. } => "role",
            Locator::Label { .. } => "label",
            Locator::TestId { .. } => "test_id",
            Locator::Text { .. } => "text",
            Locator::Css { .. } => "css",
            Locator::Xpath { .. } => "xpath",
            Locator::Auto { .. } => "auto",
        }
    }

    /// The concrete strategies `auto` expands to, in priority order.
    pub fn auto_chain(query: &str) -> Vec<Locator> {
        let q = query.to_string();
        vec![
            Locator::Role { role: "*".into(), name: Some(q.clone()), exact: true },
            Locator::Label { text: q.clone(), exact: true },
            Locator::TestId { id: q.clone() },
            Locator::Text { text: q.clone(), exact: true },
            Locator::Text { text: q, exact: false },
        ]
    }

    pub fn validate(&self) -> crate::Result<()> {
        let empty = match self {
            Locator::Ref { ref_id } => ref_id.is_empty(),
            Locator::Role { role, .. } => role.is_empty(),
            Locator::Label { text, .. } | Locator::Text { text, .. } => text.trim().is_empty(),
            Locator::TestId { id } => id.is_empty(),
            Locator::Css { selector } => selector.trim().is_empty(),
            Locator::Xpath { expr } => expr.trim().is_empty(),
            Locator::Auto { query } => query.trim().is_empty(),
        };
        if empty {
            return Err(crate::CoreError::invalid(format!("empty {} locator", self.strategy())));
        }
        Ok(())
    }
}

/// A locator plus the rule for choosing among several matches.
///
/// Selection rule: when at least one match is visible, hidden matches are
/// ignored. Without `nth`, more than one remaining match is an `ambiguous`
/// error; with `nth`, the zero-based match in document order is chosen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Target {
    #[serde(flatten)]
    pub locator: Locator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nth: Option<usize>,
}

impl From<Locator> for Target {
    fn from(locator: Locator) -> Self {
        Target { locator, nth: None }
    }
}

/// Whitespace-normalising, case-insensitive text comparison used by every backend.
pub fn text_matches(haystack: &str, needle: &str, exact: bool) -> bool {
    let h = normalize(haystack);
    let n = normalize(needle);
    if exact {
        h == n
    } else {
        h.contains(&n)
    }
}

pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_round_trips_flattened() {
        let t: Target = serde_json::from_str(r#"{"by":"role","role":"button","name":"Save","nth":1}"#).unwrap();
        assert_eq!(t.nth, Some(1));
        assert!(matches!(t.locator, Locator::Role { ref role, .. } if role == "button"));
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["by"], "role");
    }

    #[test]
    fn unknown_strategy_is_rejected() {
        assert!(serde_json::from_str::<Target>(r#"{"by":"magic","q":"x"}"#).is_err());
    }

    #[test]
    fn text_matching_normalizes() {
        assert!(text_matches("  Sign\n in ", "sign in", true));
        assert!(text_matches("Please sign in now", "SIGN IN", false));
        assert!(!text_matches("Please sign in now", "sign in", true));
    }
}
