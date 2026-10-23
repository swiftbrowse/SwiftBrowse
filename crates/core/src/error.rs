use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Classified failure kinds shared by every interface (MCP, CLI, SDKs).
///
/// The classification drives recovery: only kinds reported as recoverable may
/// be retried by the core, and only before a non-idempotent action has been
/// dispatched to the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidArgument,
    Unsupported,
    PermissionDenied,
    ApprovalRequired,
    NotFound,
    Ambiguous,
    NotActionable,
    StaleElement,
    Timeout,
    NavigationFailed,
    VerificationFailed,
    SessionNotFound,
    SessionLimit,
    Backend,
}

impl ErrorKind {
    /// Whether a later attempt can reasonably succeed without changing the request.
    pub fn recoverable(self) -> bool {
        matches!(self, ErrorKind::NotFound | ErrorKind::NotActionable | ErrorKind::StaleElement | ErrorKind::Timeout)
    }
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize, JsonSchema)]
#[error("{kind:?}: {message}")]
pub struct CoreError {
    pub kind: ErrorKind,
    pub message: String,
    pub recoverable: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl CoreError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), recoverable: kind.recoverable(), details: Value::Null }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Backend, message)
    }
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;
