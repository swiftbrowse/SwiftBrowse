//! An in-memory backend for deterministic tests of the core without a browser.
//! It models elements, visibility, timed appearance and click effects; it is
//! not a DOM.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::backend::{Backend, BackendFactory, SessionOptions};
use crate::capability::{cap, names, CapabilityReport, Support};
use crate::error::{CoreError, ErrorKind, Result};
use crate::locator::{text_matches, Locator};
use crate::model::*;
use crate::request::{SnapshotMode, WaitUntil};

#[derive(Debug, Clone)]
pub enum Effect {
    Navigate { url: String },
    Show(String),
    Hide(String),
    Remove(String),
}

#[derive(Debug, Clone)]
pub struct MockElement {
    pub info: ElementInfo,
    pub label: Option<String>,
    pub appears_after: Option<Duration>,
    pub on_click: Vec<Effect>,
}

impl MockElement {
    pub fn new(ref_id: &str, tag: &str, role: Option<&str>, name: &str) -> Self {
        Self {
            info: ElementInfo {
                ref_id: ref_id.into(),
                tag: tag.into(),
                role: role.map(Into::into),
                name: name.into(),
                text: name.into(),
                value: None,
                test_id: None,
                input_type: None,
                href: None,
                checked: None,
                visible: true,
                enabled: true,
                editable: matches!(tag, "input" | "textarea"),
            },
            label: None,
            appears_after: None,
            on_click: Vec::new(),
        }
    }
    pub fn button(ref_id: &str, name: &str) -> Self {
        Self::new(ref_id, "button", Some("button"), name)
    }
    pub fn textbox(ref_id: &str, label: &str) -> Self {
        let mut e = Self::new(ref_id, "input", Some("textbox"), label);
        e.info.text.clear();
        e.info.value = Some(String::new());
        e.label = Some(label.into());
        e
    }
    pub fn hidden(mut self) -> Self {
        self.info.visible = false;
        self
    }
    pub fn disabled(mut self) -> Self {
        self.info.enabled = false;
        self
    }
    pub fn test_id(mut self, id: &str) -> Self {
        self.info.test_id = Some(id.into());
        self
    }
    pub fn appears_after(mut self, d: Duration) -> Self {
        self.appears_after = Some(d);
        self
    }
    pub fn on_click(mut self, effect: Effect) -> Self {
        self.on_click.push(effect);
        self
    }
}

#[derive(Debug, Clone)]
pub struct MockPage {
    pub title: String,
    pub elements: Vec<MockElement>,
}

#[derive(Debug, Default)]
pub struct MockState {
    pub pages: HashMap<String, MockPage>,
    pub url: String,
    pub current: Option<MockPage>,
    pub loaded_at: Option<Instant>,
    pub clicks: Vec<String>,
    pub focused: Option<String>,
}

#[derive(Clone, Default)]
pub struct MockBackend {
    pub state: Arc<Mutex<MockState>>,
}

impl MockBackend {
    pub fn with_pages(pages: HashMap<String, MockPage>) -> Self {
        let state = MockState { pages, ..Default::default() };
        Self { state: Arc::new(Mutex::new(state)) }
    }

    fn present(st: &MockState) -> Vec<MockElement> {
        let elapsed = st.loaded_at.map(|t| t.elapsed()).unwrap_or_default();
        st.current
            .as_ref()
            .map(|p| p.elements.iter().filter(|e| e.appears_after.is_none_or(|d| elapsed >= d)).cloned().collect())
            .unwrap_or_default()
    }

    fn find(&self, ref_id: &str) -> Result<MockElement> {
        let st = self.state.lock().unwrap();
        Self::present(&st)
            .into_iter()
            .find(|e| e.info.ref_id == ref_id)
            .ok_or_else(|| CoreError::new(ErrorKind::StaleElement, format!("{ref_id} is no longer attached")))
    }

    fn actionable(&self, ref_id: &str) -> Result<MockElement> {
        let e = self.find(ref_id)?;
        if !e.info.visible {
            return Err(CoreError::new(ErrorKind::NotActionable, format!("{ref_id} is not visible")));
        }
        if !e.info.enabled {
            return Err(CoreError::new(ErrorKind::NotActionable, format!("{ref_id} is disabled")));
        }
        Ok(e)
    }

    fn update(&self, ref_id: &str, f: impl FnOnce(&mut MockElement)) {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.current.as_mut().and_then(|p| p.elements.iter_mut().find(|e| e.info.ref_id == ref_id)) {
            f(e);
        }
    }

    fn load(&self, url: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        let page = st
            .pages
            .get(url)
            .cloned()
            .ok_or_else(|| CoreError::new(ErrorKind::NavigationFailed, format!("mock has no page {url}")))?;
        st.url = url.into();
        st.current = Some(page);
        st.loaded_at = Some(Instant::now());
        Ok(())
    }
}

#[async_trait]
impl Backend for MockBackend {
    fn capabilities(&self) -> CapabilityReport {
        CapabilityReport {
            backend: "mock".into(),
            engine: "in-memory".into(),
            engine_version: None,
            headless: true,
            capabilities: vec![
                cap(names::NAVIGATE, Support::Full, None),
                cap(names::SNAPSHOT, Support::Partial, Some("no headings or messages")),
                cap(names::SEMANTIC_LOCATORS, Support::Full, None),
                cap(names::CSS_XPATH, Support::Unsupported, None),
                cap(names::SCREENSHOT, Support::Unsupported, None),
            ],
            warnings: vec!["mock backend: for tests only".into()],
        }
    }

    async fn navigate(&mut self, url: &str, _w: WaitUntil, _t: Duration) -> Result<NavigationInfo> {
        self.load(url)?;
        let title = self.page_state().await?.title;
        Ok(NavigationInfo {
            url: url.into(),
            title,
            status: Some(200),
            same_document: false,
            rendered_after_load_ms: None,
            still_empty: false,
            still_changing: false,
        })
    }

    async fn page_state(&mut self) -> Result<PageState> {
        let st = self.state.lock().unwrap();
        Ok(PageState { url: st.url.clone(), title: st.current.as_ref().map(|p| p.title.clone()).unwrap_or_default() })
    }

    async fn snapshot(&mut self, mode: SnapshotMode, max: usize) -> Result<PageSnapshot> {
        let st = self.state.lock().unwrap();
        let all: Vec<ElementInfo> = Self::present(&st)
            .into_iter()
            .map(|e| e.info)
            .filter(|e| mode == SnapshotMode::Full || e.visible)
            .collect();
        Ok(PageSnapshot {
            url: st.url.clone(),
            title: st.current.as_ref().map(|p| p.title.clone()).unwrap_or_default(),
            headings: Vec::new(),
            total_elements: all.len(),
            truncated: all.len() > max,
            elements: all.into_iter().take(max).collect(),
            messages: Vec::new(),
            rendered_after_load_ms: None,
            still_empty: false,
            still_changing: false,
        })
    }

    async fn query(&mut self, locator: &Locator) -> Result<Vec<ElementInfo>> {
        let st = self.state.lock().unwrap();
        let els = Self::present(&st);
        let hit = |e: &MockElement| match locator {
            Locator::Role { role, name, exact } => {
                let role_ok = match &e.info.role {
                    Some(r) => role == "*" || r == role,
                    None => false,
                };
                role_ok && name.as_ref().is_none_or(|n| text_matches(&e.info.name, n, *exact))
            }
            Locator::Label { text, exact } => e.label.as_ref().is_some_and(|l| text_matches(l, text, *exact)),
            Locator::TestId { id } => e.info.test_id.as_deref() == Some(id.as_str()),
            Locator::Text { text, exact } => !e.info.text.is_empty() && text_matches(&e.info.text, text, *exact),
            _ => false,
        };
        match locator {
            Locator::Css { .. } | Locator::Xpath { .. } => Err(CoreError::unsupported("mock backend has no CSS/XPath")),
            Locator::Ref { .. } | Locator::Auto { .. } => Err(CoreError::invalid("resolved by the core")),
            _ => Ok(els.iter().filter(|e| hit(e)).map(|e| e.info.clone()).collect()),
        }
    }

    async fn describe(&mut self, ref_id: &str) -> Result<ElementInfo> {
        Ok(self.find(ref_id)?.info)
    }

    async fn read_value(&mut self, ref_id: &str) -> Result<Option<String>> {
        Ok(self.find(ref_id)?.info.value)
    }

    async fn click(&mut self, ref_id: &str) -> Result<()> {
        let el = self.actionable(ref_id)?;
        self.state.lock().unwrap().clicks.push(ref_id.into());
        for effect in el.on_click {
            match effect {
                Effect::Navigate { url } => self.load(&url)?,
                Effect::Show(r) => self.update(&r, |e| e.info.visible = true),
                Effect::Hide(r) => self.update(&r, |e| e.info.visible = false),
                Effect::Remove(r) => {
                    let mut st = self.state.lock().unwrap();
                    if let Some(p) = st.current.as_mut() {
                        p.elements.retain(|e| e.info.ref_id != r);
                    }
                }
            }
        }
        Ok(())
    }

    async fn fill(&mut self, ref_id: &str, value: &str) -> Result<()> {
        let el = self.actionable(ref_id)?;
        if !el.info.editable {
            return Err(CoreError::new(ErrorKind::NotActionable, format!("{ref_id} is not editable")));
        }
        self.update(ref_id, |e| e.info.value = Some(value.into()));
        Ok(())
    }

    async fn focus(&mut self, ref_id: &str) -> Result<()> {
        self.actionable(ref_id)?;
        self.state.lock().unwrap().focused = Some(ref_id.into());
        Ok(())
    }

    async fn type_text(&mut self, text: &str) -> Result<()> {
        let focused = self.state.lock().unwrap().focused.clone();
        let r = focused.ok_or_else(|| CoreError::new(ErrorKind::NotActionable, "nothing is focused"))?;
        self.update(&r, |e| e.info.value.get_or_insert_with(String::new).push_str(text));
        Ok(())
    }

    async fn press_key(&mut self, _key: &str) -> Result<()> {
        Ok(())
    }

    async fn select_option(&mut self, _ref_id: &str, _option: &str) -> Result<Vec<String>> {
        Err(CoreError::unsupported("mock backend has no <select>"))
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

pub struct MockFactory {
    pub pages: HashMap<String, MockPage>,
    /// Shared with every backend this factory launches, for test inspection.
    pub last: Mutex<Option<MockBackend>>,
}

impl MockFactory {
    pub fn new(pages: HashMap<String, MockPage>) -> Self {
        Self { pages, last: Mutex::new(None) }
    }
}

#[async_trait]
impl BackendFactory for MockFactory {
    fn name(&self) -> &str {
        "mock"
    }
    async fn launch(&self, _o: &SessionOptions) -> Result<Box<dyn Backend>> {
        let b = MockBackend::with_pages(self.pages.clone());
        *self.last.lock().unwrap() = Some(b.clone());
        Ok(Box::new(b))
    }
}
