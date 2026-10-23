//! Minimal Chrome DevTools Protocol client over one WebSocket, using flattened
//! target sessions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use swiftbrowse_core::{CoreError, ErrorKind, Result};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

#[derive(Debug)]
pub struct Event {
    pub method: String,
    pub params: Value,
    pub session_id: Option<String>,
}

#[derive(Clone)]
pub struct Cdp {
    tx: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
}

impl Cdp {
    /// Connects and returns the client plus the stream of protocol events.
    pub async fn connect(ws_url: &str) -> Result<(Cdp, mpsc::UnboundedReceiver<Event>)> {
        let (ws, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| CoreError::backend(format!("cannot connect to DevTools at {ws_url}: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        let pending: Pending = Arc::default();

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let pending_r = pending.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                if let Some(id) = v.get("id").and_then(Value::as_u64) {
                    let Some(waiter) = pending_r.lock().unwrap().remove(&id) else { continue };
                    let result = match v.get("error") {
                        Some(err) => Err(protocol_error(err)),
                        None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = waiter.send(result);
                } else if let Some(method) = v.get("method").and_then(Value::as_str) {
                    let _ = ev_tx.send(Event {
                        method: method.to_string(),
                        params: v.get("params").cloned().unwrap_or(Value::Null),
                        session_id: v.get("sessionId").and_then(Value::as_str).map(str::to_string),
                    });
                }
            }
            for (_, waiter) in pending_r.lock().unwrap().drain() {
                let _ = waiter.send(Err(CoreError::backend("browser connection closed")));
            }
        });

        Ok((Cdp { tx, pending, next_id: Arc::new(AtomicU64::new(1)) }, ev_rx))
    }

    pub async fn send(&self, method: &str, params: Value, session: Option<&str>, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if self.tx.send(Message::Text(msg.to_string().into())).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(CoreError::backend("browser connection closed"));
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(CoreError::backend("browser connection closed")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(CoreError::new(ErrorKind::Timeout, format!("{method} did not respond within {} ms", timeout.as_millis())))
            }
        }
    }
}

fn protocol_error(err: &Value) -> CoreError {
    let message = err.get("message").and_then(Value::as_str).unwrap_or("protocol error");
    let data = err.get("data").and_then(Value::as_str).unwrap_or("");
    CoreError::backend(if data.is_empty() { message.to_string() } else { format!("{message}: {data}") })
}
