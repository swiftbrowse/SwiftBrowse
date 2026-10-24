use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::capability::CapabilityReport;
use crate::error::{CoreError, Result};
use crate::locator::Locator;
use crate::model::*;
use crate::request::{SnapshotMode, WaitUntil};

/// Options a backend receives when a session starts.
#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub headless: bool,
    pub viewport: (u32, u32),
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self { headless: true, viewport: (1280, 800) }
    }
}

/// A browser engine adapter. One instance drives one isolated session.
///
/// Contract for interaction methods (`click`, `fill`, `select_option`,
/// `focus`): they must return `NotActionable` or `StaleElement` only when no
/// input reached the page, so the core may safely re-resolve and retry. Any
/// failure after input was dispatched must use another kind.
#[async_trait]
pub trait Backend: Send {
    fn capabilities(&self) -> CapabilityReport;

    async fn navigate(&mut self, url: &str, wait_until: WaitUntil, timeout: Duration) -> Result<NavigationInfo>;
    async fn page_state(&mut self) -> Result<PageState>;
    async fn snapshot(&mut self, mode: SnapshotMode, max_elements: usize) -> Result<PageSnapshot>;

    /// All elements matching one concrete (non-`auto`) locator, in document order.
    async fn query(&mut self, locator: &Locator) -> Result<Vec<ElementInfo>>;
    /// Fresh info for a ref, or `StaleElement`.
    async fn describe(&mut self, ref_id: &str) -> Result<ElementInfo>;

    /// Raw current value, used only for verification. The core never returns it
    /// for password fields; `ElementInfo.value` must already be redacted for them.
    async fn read_value(&mut self, ref_id: &str) -> Result<Option<String>>;

    async fn click(&mut self, ref_id: &str) -> Result<()>;
    async fn fill(&mut self, ref_id: &str, value: &str) -> Result<()>;
    async fn focus(&mut self, ref_id: &str) -> Result<()>;
    async fn type_text(&mut self, text: &str) -> Result<()>;
    async fn press_key(&mut self, key: &str) -> Result<()>;
    /// Returns the selected option values after the change.
    async fn select_option(&mut self, ref_id: &str, option: &str) -> Result<Vec<String>>;

    async fn accessibility_tree(&mut self, _max_nodes: usize) -> Result<Value> {
        Err(CoreError::unsupported("accessibility tree is not available on this backend"))
    }
    async fn screenshot(&mut self, _path: &Path, _full_page: bool) -> Result<()> {
        Err(CoreError::unsupported("screenshots are not available on this backend"))
    }
    async fn console_errors(&mut self) -> Result<Vec<ConsoleEntry>> {
        Err(CoreError::unsupported("console diagnostics are not available on this backend"))
    }
    async fn network_failures(&mut self) -> Result<Vec<NetworkFailure>> {
        Err(CoreError::unsupported("network diagnostics are not available on this backend"))
    }
    async fn dialogs(&mut self) -> Result<Vec<DialogEvent>> {
        Ok(Vec::new())
    }
    /// What observably changed after the last click (`navigated`, `url_changed`,
    /// `state_changed`, `dom_changed`), waiting up to `max` for the first change.
    /// An empty list means nothing changed; `None` means the backend can't tell.
    async fn click_effect(&mut self, _max: Duration) -> Result<Option<Vec<String>>> {
        Ok(None)
    }

    async fn close(&mut self) -> Result<()>;
}

/// Starts backends for new sessions.
#[async_trait]
pub trait BackendFactory: Send + Sync {
    fn name(&self) -> &str;
    async fn launch(&self, options: &SessionOptions) -> Result<Box<dyn Backend>>;
}
