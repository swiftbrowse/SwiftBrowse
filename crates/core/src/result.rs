use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::CoreError;
use crate::model::{ElementInfo, PageState};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedTarget {
    pub element: ElementInfo,
    /// Strategy that produced the match (for `auto`, the concrete one used).
    pub strategy: String,
    /// Matches considered before applying the selection rule.
    pub candidates: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    /// The action was dispatched but no expected outcome was supplied or checkable.
    NotChecked,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Check {
    pub condition: Value,
    pub passed: bool,
    pub observed: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Verification {
    pub status: VerificationStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<Check>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Timing {
    pub started_at_ms: u64,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    pub kind: String,
    pub path: String,
    pub action_id: String,
}

/// The result shape every operation returns, on every interface.
///
/// `ok` means the operation did what it was asked *and* any requested
/// verification passed. An interaction without `expect` reports
/// `verification.status = not_checked`; callers must not read that as proof
/// that the intended outcome happened.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionResult {
    pub ok: bool,
    pub op: String,
    pub action_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ResolvedTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<Verification>,
    pub timing: Timing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CoreError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}
