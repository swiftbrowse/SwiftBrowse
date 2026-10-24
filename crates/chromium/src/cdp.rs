//! Minimal Chrome DevTools Protocol client over a pipe or a WebSocket, using
//! flattened target sessions.

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
    tx: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
}

/// Routes one incoming protocol message to its waiter or the event stream.
fn dispatch(text: &str, pending: &Pending, events: &mpsc::UnboundedSender<Event>) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    if let Some(id) = v.get("id").and_then(Value::as_u64) {
        let Some(waiter) = pending.lock().unwrap().remove(&id) else { return };
        let result = match v.get("error") {
            Some(err) => Err(protocol_error(err)),
            None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = waiter.send(result);
    } else if let Some(method) = v.get("method").and_then(Value::as_str) {
        let _ = events.send(Event {
            method: method.to_string(),
            params: v.get("params").cloned().unwrap_or(Value::Null),
            session_id: v.get("sessionId").and_then(Value::as_str).map(str::to_string),
        });
    }
}

fn fail_pending(pending: &Pending) {
    for (_, waiter) in pending.lock().unwrap().drain() {
        let _ = waiter.send(Err(CoreError::backend("browser connection closed")));
    }
}

impl Cdp {
    fn new_parts() -> (Cdp, mpsc::UnboundedReceiver<String>, Pending) {
        let (tx, rx) = mpsc::unbounded_channel::<String>();
        let pending: Pending = Arc::default();
        (Cdp { tx, pending: pending.clone(), next_id: Arc::new(AtomicU64::new(1)) }, rx, pending)
    }

    /// Connects over WebSocket and returns the client plus the stream of protocol events.
    pub async fn connect(ws_url: &str) -> Result<(Cdp, mpsc::UnboundedReceiver<Event>)> {
        let (ws, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| CoreError::backend(format!("cannot connect to DevTools at {ws_url}: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        let (cdp, mut rx, pending) = Self::new_parts();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sink.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                if let Message::Text(text) = msg {
                    dispatch(text.as_str(), &pending, &ev_tx);
                }
            }
            fail_pending(&pending);
        });
        Ok((cdp, ev_rx))
    }

    /// Talks NUL-terminated JSON over a byte stream: `--remote-debugging-pipe`
    /// or the warm-browser daemon's socket. Commands can be written before the
    /// other side is ready; it reads them as soon as it can.
    pub fn over_stream<W, R>(mut to_browser: W, mut from_browser: R) -> (Cdp, mpsc::UnboundedReceiver<Event>)
    where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (cdp, mut rx, pending) = Self::new_parts();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let mut bytes = msg.into_bytes();
                bytes.push(0);
                if to_browser.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                match from_browser.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        let mut start = 0;
                        while let Some(end) = buf[start..].iter().position(|b| *b == 0) {
                            if let Ok(text) = std::str::from_utf8(&buf[start..start + end]) {
                                dispatch(text, &pending, &ev_tx);
                            }
                            start += end + 1;
                        }
                        buf.drain(..start);
                    }
                }
            }
            fail_pending(&pending);
        });
        (cdp, ev_rx)
    }

    /// Queues a command now and returns its reply as a future. Commands on one
    /// session are dispatched in the order they were queued, so later commands
    /// can be sent without waiting for earlier replies.
    pub fn enqueue(&self, method: &str, params: Value, session: Option<&str>) -> Pending1 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if self.tx.send(msg.to_string()).is_err() {
            self.pending.lock().unwrap().remove(&id);
        }
        Pending1 { id, method: method.to_string(), rx, pending: self.pending.clone() }
    }

    pub async fn send(&self, method: &str, params: Value, session: Option<&str>, timeout: Duration) -> Result<Value> {
        self.enqueue(method, params, session).wait(timeout).await
    }
}

/// A queued command's reply.
pub struct Pending1 {
    id: u64,
    method: String,
    rx: oneshot::Receiver<Result<Value>>,
    pending: Pending,
}

impl Pending1 {
    pub async fn wait(self, timeout: Duration) -> Result<Value> {
        let Pending1 { id, method, rx, pending } = self;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(CoreError::backend("browser connection closed")),
            Err(_) => {
                pending.lock().unwrap().remove(&id);
                Err(CoreError::new(
                    ErrorKind::Timeout,
                    format!("{method} did not respond within {} ms", timeout.as_millis()),
                ))
            }
        }
    }
}

fn protocol_error(err: &Value) -> CoreError {
    let message = err.get("message").and_then(Value::as_str).unwrap_or("protocol error");
    let data = err.get("data").and_then(Value::as_str).unwrap_or("");
    CoreError::backend(if data.is_empty() { message.to_string() } else { format!("{message}: {data}") })
}
