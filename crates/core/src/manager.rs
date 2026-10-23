use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;

use crate::backend::{BackendFactory, SessionOptions};
use crate::error::{CoreError, ErrorKind, Result};
use crate::model::now_ms;
use crate::policy::Policy;
use crate::request::Request;
use crate::result::{ActionResult, Timing};
use crate::session::Session;

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub backend: String,
    pub engine: String,
    pub engine_version: Option<String>,
}

/// Owns live sessions. Each session gets its own backend instance (and, for
/// browser backends, its own profile), so cookies and storage never leak
/// between sessions.
pub struct SessionManager {
    factory: Arc<dyn BackendFactory>,
    policy: Arc<Policy>,
    sessions: BTreeMap<String, Session>,
    pub max_sessions: usize,
    next_id: u64,
}

impl SessionManager {
    pub fn new(factory: Arc<dyn BackendFactory>, policy: Policy) -> Self {
        Self { factory, policy: Arc::new(policy), sessions: BTreeMap::new(), max_sessions: 8, next_id: 0 }
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    pub async fn create(&mut self, id: Option<String>, options: &SessionOptions) -> Result<SessionInfo> {
        if self.sessions.len() >= self.max_sessions {
            return Err(CoreError::new(ErrorKind::SessionLimit, format!("at most {} sessions", self.max_sessions)));
        }
        let id = match id {
            Some(id) => {
                let valid = !id.is_empty()
                    && id.len() <= 64
                    && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                if !valid {
                    return Err(CoreError::invalid("session_id may only contain letters, digits, '-' and '_'"));
                }
                if self.sessions.contains_key(&id) {
                    return Err(CoreError::invalid(format!("session {id} already exists")));
                }
                id
            }
            None => loop {
                self.next_id += 1;
                let id = format!("s{}", self.next_id);
                if !self.sessions.contains_key(&id) {
                    break id;
                }
            },
        };
        let backend = self.factory.launch(options).await?;
        let session = Session::new(id.clone(), backend, self.policy.clone());
        let info = info_of(&session);
        self.sessions.insert(id, session);
        Ok(info)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.sessions.contains_key(id)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions.values().map(info_of).collect()
    }

    pub async fn close(&mut self, id: &str) -> Result<()> {
        let mut session = self
            .sessions
            .remove(id)
            .ok_or_else(|| CoreError::new(ErrorKind::SessionNotFound, format!("no session {id}")))?;
        session.close().await
    }

    pub async fn close_all(&mut self) {
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let _ = self.close(&id).await;
        }
    }

    pub async fn execute(&mut self, id: &str, request: Request) -> ActionResult {
        match self.sessions.get_mut(id) {
            Some(session) => session.execute(request).await,
            None => ActionResult {
                ok: false,
                op: request.op().into(),
                action_id: String::new(),
                session_id: id.into(),
                target: None,
                page: None,
                verification: None,
                timing: Timing { started_at_ms: now_ms(), duration_ms: 0, timeout_ms: None },
                data: None,
                error: Some(
                    CoreError::new(ErrorKind::SessionNotFound, format!("no session {id}"))
                        .with_details(json!({ "open_sessions": self.sessions.keys().collect::<Vec<_>>() })),
                ),
                evidence: Vec::new(),
                notes: Vec::new(),
            },
        }
    }
}

fn info_of(s: &Session) -> SessionInfo {
    let caps = s.capabilities();
    SessionInfo {
        session_id: s.id().to_string(),
        backend: caps.backend.clone(),
        engine: caps.engine.clone(),
        engine_version: caps.engine_version.clone(),
    }
}
