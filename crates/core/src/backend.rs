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

    /// Moves the mouse over the element. Same retry contract as `click`.
    async fn hover(&mut self, _ref_id: &str) -> Result<()> {
        Err(CoreError::unsupported("hover is not available on this backend"))
    }
    /// The element's full visible text, whitespace-normalized.
    async fn read_text(&mut self, ref_id: &str) -> Result<String> {
        Ok(self.describe(ref_id).await?.text)
    }
    /// Goes back (-1), forward (1) or reloads (0) and waits for the page to load.
    async fn history(&mut self, _delta: i32, _timeout: Duration) -> Result<NavigationInfo> {
        Err(CoreError::unsupported("history navigation is not available on this backend"))
    }
    async fn evaluate(&mut self, _expression: &str, _timeout: Duration) -> Result<Value> {
        Err(CoreError::unsupported("script evaluation is not available on this backend"))
    }

    /// Number of open tabs, without asking the browser.
    fn tab_count(&self) -> usize {
        1
    }
    /// Sets the files of an `<input type=file>` and returns the selected file names.
    async fn upload_files(&mut self, _ref_id: &str, _files: &[std::path::PathBuf]) -> Result<Vec<String>> {
        Err(CoreError::unsupported("file upload is not available on this backend"))
    }
    /// Open tabs, in creation order.
    async fn tabs(&mut self) -> Result<Vec<TabInfo>> {
        Err(CoreError::unsupported("tabs are not available on this backend"))
    }
    /// Opens a blank tab and makes it the active one.
    async fn new_tab(&mut self) -> Result<()> {
        Err(CoreError::unsupported("tabs are not available on this backend"))
    }
    async fn switch_tab(&mut self, _index: usize) -> Result<()> {
        Err(CoreError::unsupported("tabs are not available on this backend"))
    }
    /// Closes a tab; when it was active, the opener (or the last tab) becomes active.
    async fn close_tab(&mut self, _index: usize) -> Result<()> {
        Err(CoreError::unsupported("tabs are not available on this backend"))
    }
    /// Every frame of the active tab, main frame first.
    async fn frames(&mut self) -> Result<Vec<FrameInfo>> {
        Err(CoreError::unsupported("frames are not available on this backend"))
    }
    /// Makes later operations act inside this frame (`None`: back to the main frame).
    async fn enter_frame(&mut self, _frame: Option<FrameRef>) -> Result<FrameInfo> {
        Err(CoreError::unsupported("frames are not available on this backend"))
    }

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

/// Which frame to enter.
#[derive(Debug, Clone)]
pub enum FrameRef {
    /// The `<iframe>` element with this ref, in the current frame.
    Element(String),
    /// A frame id from [`Backend::frames`].
    Id(String),
}

/// Starts backends for new sessions.
#[async_trait]
pub trait BackendFactory: Send + Sync {
    fn name(&self) -> &str;
    async fn launch(&self, options: &SessionOptions) -> Result<Box<dyn Backend>>;
}
