//! `swiftbrowse sdk`: the protocol the TypeScript and Python SDKs speak.
//!
//! One JSON object per line on stdin, one per line on stdout:
//!
//! ```text
//! → {"id": 1, "method": "create_session", "params": {"headless": true, "viewport": [1280, 800]}}
//! ← {"id": 1, "result": {"session_id": "s1", ...}}
//! → {"id": 2, "method": "execute", "params": {"session": "s1", "op": "navigate", "args": {"url": "…"}}}
//! ← {"id": 2, "result": <ActionResult>}
//! → {"id": 3, "method": "close_session", "params": {"session": "s1"}}
//! → {"id": 4, "method": "ops"}            // every operation with its JSON schema
//! ```
//!
//! Each session has its own manager, so requests to different sessions run
//! concurrently; requests to one session run in order.

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use swiftbrowse_core::{Request, SessionManager, SessionOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

use super::Common;

/// Protocol version; the SDKs check it.
pub const PROTOCOL: u32 = 1;

type Sessions = Arc<std::sync::Mutex<HashMap<String, Arc<Mutex<SessionManager>>>>>;

pub async fn serve(common: Common) -> ExitCode {
    let sessions: Sessions = Arc::default();
    let next = Arc::new(AtomicU64::new(0));
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = out_rx.recv().await {
            if stdout.write_all(line.as_bytes()).await.is_err() || stdout.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut tasks = tokio::task::JoinSet::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let (common, sessions, next, out) = (common.clone(), sessions.clone(), next.clone(), out_tx.clone());
        tasks.spawn(async move {
            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    let _ = out
                        .send(json!({ "id": null, "error": { "message": format!("invalid JSON: {e}") } }).to_string());
                    return;
                }
            };
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let reply = match handle(&common, &sessions, &next, &msg).await {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(message) => json!({ "id": id, "error": { "message": message } }),
            };
            let _ = out.send(reply.to_string());
        });
    }
    // stdin closed: the SDK process is gone or done. Close every session.
    while tasks.join_next().await.is_some() {}
    let all: Vec<_> = sessions.lock().unwrap().drain().map(|(_, m)| m).collect();
    for m in all {
        m.lock().await.close_all().await;
    }
    drop(out_tx);
    let _ = writer.await;
    ExitCode::SUCCESS
}

async fn handle(common: &Common, sessions: &Sessions, next: &AtomicU64, msg: &Value) -> Result<Value, String> {
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let session_of = |p: &Value| -> Result<Arc<Mutex<SessionManager>>, String> {
        let id = p["session"].as_str().ok_or("missing params.session")?;
        sessions.lock().unwrap().get(id).cloned().ok_or_else(|| format!("no session {id}"))
    };
    match msg.get("method").and_then(Value::as_str).unwrap_or_default() {
        "hello" => Ok(json!({ "protocol": PROTOCOL, "version": env!("CARGO_PKG_VERSION") })),
        "ops" => Ok(Value::Array(
            swiftbrowse_core::request::op_specs()
                .into_iter()
                .map(|s| json!({ "name": s.name, "description": s.description, "schema": (s.schema)() }))
                .collect(),
        )),
        "create_session" => {
            let mut options = SessionOptions { headless: !common.headed, ..Default::default() };
            if let Some(h) = params["headless"].as_bool() {
                options.headless = h;
            }
            if let Some([w, h]) = params["viewport"].as_array().map(|v| v.as_slice()) {
                options.viewport = (w.as_u64().unwrap_or(1280) as u32, h.as_u64().unwrap_or(800) as u32);
            }
            let n = next.fetch_add(1, Ordering::Relaxed) + 1;
            let id = format!("s{n}");
            let mut mgr = common.manager_with(common.policy());
            let info = mgr.create(Some(id.clone()), &options).await.map_err(|e| e.message)?;
            sessions.lock().unwrap().insert(id, Arc::new(Mutex::new(mgr)));
            Ok(serde_json::to_value(info).unwrap_or_default())
        }
        "execute" => {
            let mgr = session_of(&params)?;
            let op = params["op"].as_str().ok_or("missing params.op")?;
            let request =
                Request::from_op(op, params.get("args").cloned().unwrap_or(json!({}))).map_err(|e| e.message)?;
            let id = params["session"].as_str().unwrap_or_default().to_string();
            let result = mgr.lock().await.execute(&id, request).await;
            Ok(serde_json::to_value(result).unwrap_or_default())
        }
        "close_session" => {
            let id = params["session"].as_str().ok_or("missing params.session")?.to_string();
            let mgr = sessions.lock().unwrap().remove(&id);
            if let Some(mgr) = mgr {
                mgr.lock().await.close_all().await;
            }
            Ok(json!({}))
        }
        other => Err(format!("unknown method {other:?}")),
    }
}
