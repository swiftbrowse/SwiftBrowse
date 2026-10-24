//! Warm-browser daemon (Linux, macOS, Windows).
//!
//! One long-lived headless browser shared by many SwiftBrowse clients. Clients
//! connect over a local endpoint and speak plain CDP (NUL-delimited JSON); the
//! daemon multiplexes them onto its browser connection:
//!
//! - every client works only on browser contexts, tabs and sessions it created
//!   or claimed; browser-wide commands such as `Browser.close` are refused;
//! - a spare tab, in its own fresh context with its session already set up, is
//!   kept ready; `SwiftBrowse.claimSpare` hands it over and a new one is made;
//! - a client's contexts (cookies, storage, cache) are disposed when it
//!   disconnects;
//! - the daemon exits after an idle timeout, when the browser dies, or on
//!   `SwiftBrowse.shutdown`; the next client starts a fresh one.
//!
//! Access control: clients must present a random token from a file only this
//! user can read. On Unix the endpoint is a socket in a `0700` directory and the
//! peer's uid is checked too; on Windows it is a named pipe that rejects remote
//! clients.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use swiftbrowse_core::{CoreError, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

use crate::launch;

pub const ENV_SOCKET: &str = "SWIFTBROWSE_DAEMON_SOCKET";

pub type BoxRead = Box<dyn AsyncRead + Unpin + Send>;
pub type BoxWrite = Box<dyn AsyncWrite + Unpin + Send>;

// ---------- endpoint, token and state files ----------

/// Where the daemon listens: a socket path (Unix) or pipe name (Windows),
/// versioned so a client never talks to a daemon from another build.
/// Identifies what a daemon prepares for its clients: the version plus a hash of
/// the page helper it installs, so a client never attaches to a daemon from a
/// different build.
fn build_id() -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in crate::HELPER.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{}-{:08x}", env!("CARGO_PKG_VERSION"), h as u32)
}

pub fn endpoint() -> String {
    if let Some(p) = std::env::var_os(ENV_SOCKET) {
        return p.to_string_lossy().into_owned();
    }
    #[cfg(unix)]
    {
        state_dir().join(format!("daemon-{}.sock", build_id())).display().to_string()
    }
    #[cfg(windows)]
    {
        let user: String = std::env::var("USERNAME")
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!(r"\\.\pipe\swiftbrowse-{user}-{}", build_id())
    }
}

/// User-private directory for the token, lock and log files (and the socket on Unix).
fn state_dir() -> PathBuf {
    #[cfg(unix)]
    {
        if let Some(p) = std::env::var_os(ENV_SOCKET) {
            if let Some(parent) = Path::new(&p).parent() {
                return parent.to_path_buf();
            }
        }
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(runtime) => PathBuf::from(runtime).join("swiftbrowse"),
            None => std::env::temp_dir().join(format!("swiftbrowse-{}", unsafe { libc::geteuid() })),
        }
    }
    #[cfg(windows)]
    {
        // %LOCALAPPDATA% is readable by this user, SYSTEM and administrators only.
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("swiftbrowse")
    }
}

/// A state file next to the endpoint, e.g. `daemon-0.1.0.token`.
fn state_file(ext: &str) -> PathBuf {
    let name: String = endpoint()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("daemon")
        .trim_end_matches(".sock")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    state_dir().join(format!("{name}.{ext}"))
}

/// Creates the state directory, accessible to this user only.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        if std::fs::metadata(dir)?.uid() != unsafe { libc::geteuid() } {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "state directory owned by another user",
            ));
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    {
        std::fs::create_dir_all(dir)
    }
}

fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    std::io::Write::write_all(&mut opts.open(path)?, contents.as_bytes())
}

fn new_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| CoreError::backend(format!("no randomness for daemon token: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

// ---------- platform transport ----------

#[cfg(unix)]
mod ipc {
    use super::*;
    use tokio::net::{UnixListener, UnixStream};

    fn same_user(stream: &UnixStream) -> bool {
        stream.peer_cred().is_ok_and(|c| c.uid() == unsafe { libc::geteuid() })
    }

    pub async fn connect_raw() -> Option<(BoxRead, BoxWrite)> {
        let stream = UnixStream::connect(endpoint()).await.ok()?;
        if !same_user(&stream) {
            return None;
        }
        let (r, w) = stream.into_split();
        Some((Box::new(r), Box::new(w)))
    }

    /// True when no daemon holds this endpoint's lock (none is running or starting).
    pub fn endpoint_free() -> bool {
        use std::os::fd::AsRawFd;
        let Ok(lock) = std::fs::OpenOptions::new().read(true).open(state_file("lock")) else { return true };
        // The lock is released when `lock` is dropped.
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
    }

    /// Holds the single-daemon lock for as long as it lives.
    pub struct Listener {
        listener: UnixListener,
        _lock: std::fs::File,
    }

    impl Listener {
        /// `Ok(None)` when another daemon already holds the endpoint.
        pub fn bind() -> Result<Option<Self>> {
            use std::os::fd::AsRawFd;
            let lock_path = state_file("lock");
            let lock = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lock_path)
                .map_err(|e| CoreError::backend(format!("cannot open {}: {e}", lock_path.display())))?;
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Ok(None);
            }
            let path = endpoint();
            let _ = std::fs::remove_file(&path);
            let listener =
                UnixListener::bind(&path).map_err(|e| CoreError::backend(format!("cannot listen on {path}: {e}")))?;
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            Ok(Some(Self { listener, _lock: lock }))
        }

        pub async fn accept(&mut self) -> std::io::Result<Option<(BoxRead, BoxWrite)>> {
            let (stream, _) = self.listener.accept().await?;
            if !same_user(&stream) {
                return Ok(None);
            }
            let (r, w) = stream.into_split();
            Ok(Some((Box::new(r), Box::new(w))))
        }

        pub fn remove(&self) {
            let _ = std::fs::remove_file(endpoint());
        }
    }

    /// Starts the daemon detached from the caller's session and signals.
    pub fn spawn_detached(cmd: &mut std::process::Command) -> std::io::Result<()> {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        cmd.spawn().map(|_| ())
    }
}

#[cfg(windows)]
mod ipc {
    use super::*;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

    const ERROR_PIPE_BUSY: i32 = 231;

    pub async fn connect_raw() -> Option<(BoxRead, BoxWrite)> {
        let name = endpoint();
        let deadline = Instant::now() + Duration::from_secs(2);
        let client = loop {
            match ClientOptions::new().open(&name) {
                Ok(c) => break c,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Err(_) => return None,
            }
        };
        let (r, w) = tokio::io::split(client);
        Some((Box::new(r), Box::new(w)))
    }

    /// True when no daemon owns the pipe name. A probe connection that sends no
    /// hello is simply dropped by the daemon.
    pub fn endpoint_free() -> bool {
        match ClientOptions::new().open(endpoint()) {
            Ok(_) => false,
            Err(e) => e.raw_os_error() != Some(ERROR_PIPE_BUSY),
        }
    }

    pub struct Listener {
        name: String,
        next: NamedPipeServer,
    }

    impl Listener {
        /// `Ok(None)` when another daemon already owns the pipe name.
        pub fn bind() -> Result<Option<Self>> {
            let name = endpoint();
            match ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name) {
                Ok(next) => Ok(Some(Self { name, next })),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(None),
                Err(e) => Err(CoreError::backend(format!("cannot create pipe {name}: {e}"))),
            }
        }

        pub async fn accept(&mut self) -> std::io::Result<Option<(BoxRead, BoxWrite)>> {
            self.next.connect().await?;
            let fresh = ServerOptions::new().reject_remote_clients(true).create(&self.name)?;
            let connected = std::mem::replace(&mut self.next, fresh);
            let (r, w) = tokio::io::split(connected);
            Ok(Some((Box::new(r), Box::new(w))))
        }

        pub fn remove(&self) {}
    }

    /// Starts the daemon detached from the caller's console.
    pub fn spawn_detached(cmd: &mut std::process::Command) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).spawn().map(|_| ())
    }
}

// ---------- client side ----------

/// Connects to a running daemon and authenticates with its token.
pub async fn connect() -> Option<(BoxRead, BoxWrite)> {
    let token = std::fs::read_to_string(state_file("token")).ok()?;
    let (mut r, mut w) = ipc::connect_raw().await?;
    let mut hello =
        json!({ "id": 0, "method": "SwiftBrowse.hello", "params": { "token": token.trim() } }).to_string().into_bytes();
    hello.push(0);
    w.write_all(&hello).await.ok()?;
    let reply = read_one(&mut r).await?;
    reply.get("result").is_some().then_some((r, w))
}

async fn read_one(r: &mut BoxRead) -> Option<Value> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    // Byte-wise until the NUL so nothing after the reply is consumed here.
    loop {
        if r.read(&mut byte).await.ok()? == 0 {
            return None;
        }
        if byte[0] == 0 {
            return serde_json::from_slice(&buf).ok();
        }
        buf.push(byte[0]);
    }
}

/// One request to the daemon itself (`SwiftBrowse.status`, `SwiftBrowse.shutdown`).
pub async fn call(method: &str) -> Option<Value> {
    let (mut r, mut w) = connect().await?;
    let mut msg = json!({ "id": 1, "method": method }).to_string().into_bytes();
    msg.push(0);
    w.write_all(&msg).await.ok()?;
    let reply = read_one(&mut r).await?;
    Some(reply.get("result").cloned().unwrap_or_else(|| json!({ "error": reply.get("error") })))
}

/// Sends one raw message after authenticating and returns the reply (diagnostics, tests).
pub async fn raw(msg: Value) -> Option<Value> {
    let (mut r, mut w) = connect().await?;
    let mut bytes = msg.to_string().into_bytes();
    bytes.push(0);
    w.write_all(&bytes).await.ok()?;
    read_one(&mut r).await
}

/// Starts a daemon in the background so it outlives the process that started it.
pub fn spawn_detached(program: &Path, args: &[String]) -> std::io::Result<()> {
    private_dir(&state_dir())?;
    let stderr = std::fs::OpenOptions::new().create(true).append(true).open(state_file("log"))?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(stderr);
    ipc::spawn_detached(&mut cmd)
}

/// Connects to the daemon, starting it first when `spawn` says how.
pub async fn connect_or_spawn(spawn: Option<&(PathBuf, Vec<String>)>) -> Option<(BoxRead, BoxWrite)> {
    if let Some(c) = connect().await {
        return Some(c);
    }
    let (program, args) = spawn?;
    spawn_detached(program, args).ok()?;
    let mut spawned_at = Instant::now();
    let deadline = spawned_at + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(c) = connect().await {
            return Some(c);
        }
        // The daemon we started exits at once if an old one (still shutting
        // down) held the endpoint. Once that one is gone, start another.
        if spawned_at.elapsed() > Duration::from_millis(250) && ipc::endpoint_free() {
            spawn_detached(program, args).ok()?;
            spawned_at = Instant::now();
        }
        tokio::time::sleep(Duration::from_millis(3)).await;
    }
    None
}

/// Asks the daemon to shut down and waits until it has released its endpoint,
/// so a new daemon can start right away. Returns whether one was running.
pub async fn stop(timeout: Duration) -> bool {
    let was_running = call("SwiftBrowse.shutdown").await.is_some();
    let deadline = Instant::now() + timeout;
    while was_running && !ipc::endpoint_free() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    was_running
}

// ---------- daemon core ----------
pub struct DaemonOptions {
    pub executable: Option<PathBuf>,
    pub idle_timeout: Duration,
    pub no_sandbox: bool,
}

enum Route {
    Client { client: u64, id: Value, method: String },
    Internal(oneshot::Sender<Value>),
}

#[derive(Default)]
struct State {
    next_id: u64,
    next_client: u64,
    routes: HashMap<u64, Route>,
    clients: HashMap<u64, mpsc::UnboundedSender<String>>,
    session_owner: HashMap<String, u64>,
    target_owner: HashMap<String, u64>,
    context_owner: HashMap<String, u64>,
    spare: Option<(String, String, String)>,
    spare_pending: bool,
    claim_waiters: Vec<(u64, Value)>,
    last_activity: Option<Instant>,
    sessions_served: u64,
}

#[derive(Clone)]
struct Daemon {
    state: Arc<Mutex<State>>,
    browser: mpsc::UnboundedSender<String>,
    shutdown: Arc<tokio::sync::Notify>,
    started: Instant,
    version: Arc<Mutex<String>>,
}

impl Daemon {
    fn touch(&self) {
        self.state.lock().unwrap().last_activity = Some(Instant::now());
    }

    /// Sends a command on the daemon's own behalf.
    async fn internal(&self, method: &str, params: Value) -> Result<Value> {
        self.internal_on(None, method, params).await
    }

    async fn internal_on(&self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut st = self.state.lock().unwrap();
            st.next_id += 1;
            let id = st.next_id;
            st.routes.insert(id, Route::Internal(tx));
            id
        };
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let _ = self.browser.send(msg.to_string());
        let reply = tokio::time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| CoreError::backend(format!("{method}: browser did not reply")))?
            .map_err(|_| CoreError::backend("browser connection closed"))?;
        match reply.get("error") {
            Some(e) => Err(CoreError::backend(format!("{method}: {e}"))),
            None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
        }
    }

    /// Keeps one spare tab ready in a fresh context ("refresh" after each claim).
    fn refill_spare(&self) {
        {
            let mut st = self.state.lock().unwrap();
            if st.spare.is_some() || st.spare_pending {
                return;
            }
            st.spare_pending = true;
        }
        let d = self.clone();
        tokio::spawn(async move {
            let made = async {
                let ctx = d.internal("Target.createBrowserContext", json!({})).await?;
                let ctx = ctx["browserContextId"].as_str().unwrap_or_default().to_string();
                let target =
                    d.internal("Target.createTarget", json!({ "url": "about:blank", "browserContextId": ctx })).await?;
                let target = target["targetId"].as_str().unwrap_or_default().to_string();
                // Attach and set the session up now, so the client that claims it
                // can navigate straight away.
                let attached =
                    d.internal("Target.attachToTarget", json!({ "targetId": target, "flatten": true })).await?;
                let session = attached["sessionId"].as_str().unwrap_or_default().to_string();
                let setup = crate::session_setup(swiftbrowse_core::SessionOptions::default().viewport);
                let replies =
                    futures_util::future::join_all(setup.into_iter().map(|(m, p)| d.internal_on(Some(&session), m, p)))
                        .await;
                replies.into_iter().collect::<Result<Vec<_>>>()?;
                Ok::<_, CoreError>((ctx, target, session))
            }
            .await;
            let waiters = {
                let mut st = d.state.lock().unwrap();
                st.spare_pending = false;
                if let Ok(spare) = made {
                    st.spare = Some(spare);
                }
                std::mem::take(&mut st.claim_waiters)
            };
            for (client, id) in waiters {
                d.claim(client, id);
            }
        });
    }

    fn reply(&self, client: u64, msg: Value) {
        if let Some(tx) = self.state.lock().unwrap().clients.get(&client) {
            let _ = tx.send(msg.to_string());
        }
    }

    fn claim(&self, client: u64, id: Value) {
        let taken = {
            let mut st = self.state.lock().unwrap();
            if !st.clients.contains_key(&client) {
                return;
            }
            match st.spare.take() {
                Some((ctx, target, session)) => {
                    st.context_owner.insert(ctx.clone(), client);
                    st.target_owner.insert(target.clone(), client);
                    st.session_owner.insert(session.clone(), client);
                    st.sessions_served += 1;
                    Some((ctx, target, session))
                }
                None if st.spare_pending => {
                    st.claim_waiters.push((client, id.clone()));
                    return;
                }
                None => None,
            }
        };
        let result = match &taken {
            Some((ctx, target, session)) => {
                json!({ "browserContextId": ctx, "targetId": target, "sessionId": session })
            }
            None => json!({}),
        };
        self.reply(client, json!({ "id": id, "result": result }));
        self.refill_spare();
    }

    /// Checks a client command and forwards it, or answers it here.
    fn on_client_message(&self, client: u64, text: &str) {
        self.touch();
        let Ok(msg) = serde_json::from_str::<Value>(text) else { return };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let deny = |why: &str| {
            self.reply(
                client,
                json!({ "id": id, "error": { "code": -32000, "message": format!("swiftbrowse daemon: {why}") } }),
            )
        };
        match method.as_str() {
            "SwiftBrowse.claimSpare" => return self.claim(client, id),
            "SwiftBrowse.status" => {
                let st = self.state.lock().unwrap();
                let status = json!({
                    "pid": std::process::id(),
                    "clients": st.clients.len(),
                    "sessions_served": st.sessions_served,
                    "spare_ready": st.spare.is_some(),
                    "uptime_ms": self.started.elapsed().as_millis() as u64,
                    "browser": *self.version.lock().unwrap(),
                });
                drop(st);
                return self.reply(client, json!({ "id": id, "result": status }));
            }
            "SwiftBrowse.shutdown" => {
                self.reply(client, json!({ "id": id, "result": {} }));
                self.shutdown.notify_one();
                return;
            }
            _ => {}
        }
        {
            let st = self.state.lock().unwrap();
            let owns_ctx = |p: &str| params[p].as_str().is_some_and(|c| st.context_owner.get(c) == Some(&client));
            let owns_target = |p: &str| params[p].as_str().is_some_and(|t| st.target_owner.get(t) == Some(&client));
            let allowed = match msg.get("sessionId").and_then(Value::as_str) {
                Some(session) => st.session_owner.get(session) == Some(&client),
                None => match method.as_str() {
                    "Browser.getVersion" | "Target.createBrowserContext" => true,
                    "Target.createTarget" => owns_ctx("browserContextId"),
                    "Target.attachToTarget" | "Target.closeTarget" => owns_target("targetId"),
                    "Target.disposeBrowserContext" => owns_ctx("browserContextId"),
                    _ => false,
                },
            };
            if !allowed {
                drop(st);
                return deny(&format!("{method} is not allowed for this client"));
            }
        }
        let global = {
            let mut st = self.state.lock().unwrap();
            st.next_id += 1;
            let g = st.next_id;
            st.routes.insert(g, Route::Client { client, id, method: method.clone() });
            g
        };
        let mut out = msg;
        out["id"] = json!(global);
        if method == "Target.createBrowserContext" {
            self.state.lock().unwrap().sessions_served += 1;
        }
        let _ = self.browser.send(out.to_string());
    }

    /// Routes one browser message to the client (or internal waiter) it belongs to.
    fn on_browser_message(&self, text: &str) {
        let Ok(mut msg) = serde_json::from_str::<Value>(text) else { return };
        if let Some(global) = msg.get("id").and_then(Value::as_u64) {
            let route = self.state.lock().unwrap().routes.remove(&global);
            match route {
                Some(Route::Internal(tx)) => {
                    let _ = tx.send(msg);
                }
                Some(Route::Client { client, id, method }) => {
                    {
                        let mut st = self.state.lock().unwrap();
                        let r = &msg["result"];
                        match method.as_str() {
                            "Target.createBrowserContext" => {
                                if let Some(c) = r["browserContextId"].as_str() {
                                    st.context_owner.insert(c.to_string(), client);
                                }
                            }
                            "Target.createTarget" => {
                                if let Some(t) = r["targetId"].as_str() {
                                    st.target_owner.insert(t.to_string(), client);
                                }
                            }
                            "Target.attachToTarget" => {
                                if let Some(s) = r["sessionId"].as_str() {
                                    st.session_owner.insert(s.to_string(), client);
                                }
                            }
                            _ => {}
                        }
                    }
                    msg["id"] = id;
                    self.reply(client, msg);
                }
                None => {}
            }
            return;
        }
        let session = msg.get("sessionId").and_then(Value::as_str).map(str::to_string);
        let st = self.state.lock().unwrap();
        match session {
            Some(s) => {
                if let Some(tx) = st.session_owner.get(&s).and_then(|c| st.clients.get(c)) {
                    let _ = tx.send(text.to_string());
                }
            }
            None => {
                // Browser-level events only concern a client's own targets.
                let target = msg["params"]["targetInfo"]["targetId"]
                    .as_str()
                    .or_else(|| msg["params"]["targetId"].as_str())
                    .map(str::to_string);
                if let Some(owner) = target.and_then(|t| st.target_owner.get(&t).copied()) {
                    if let Some(tx) = st.clients.get(&owner) {
                        let _ = tx.send(text.to_string());
                    }
                }
            }
        }
    }

    async fn disconnected(&self, client: u64) {
        let contexts: Vec<String> = {
            let mut st = self.state.lock().unwrap();
            st.clients.remove(&client);
            st.session_owner.retain(|_, c| *c != client);
            st.target_owner.retain(|_, c| *c != client);
            st.claim_waiters.retain(|(c, _)| *c != client);
            let mine: Vec<String> =
                st.context_owner.iter().filter(|(_, c)| **c == client).map(|(k, _)| k.clone()).collect();
            st.context_owner.retain(|_, c| *c != client);
            st.last_activity = Some(Instant::now());
            mine
        };
        for ctx in contexts {
            let _ = self.internal("Target.disposeBrowserContext", json!({ "browserContextId": ctx })).await;
        }
    }
}

/// Splits NUL-delimited messages out of a byte stream. `each` returns false to stop.
async fn read_messages<R: AsyncRead + Unpin>(mut r: R, mut each: impl FnMut(&str) -> bool) {
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                let mut start = 0;
                while let Some(end) = buf[start..].iter().position(|b| *b == 0) {
                    if let Ok(text) = std::str::from_utf8(&buf[start..start + end]) {
                        if !each(text) {
                            return;
                        }
                    }
                    start += end + 1;
                }
                buf.drain(..start);
            }
        }
    }
}

async fn write_messages<W: AsyncWrite + Unpin>(mut w: W, mut rx: mpsc::UnboundedReceiver<String>) {
    while let Some(msg) = rx.recv().await {
        let mut bytes = msg.into_bytes();
        bytes.push(0);
        if w.write_all(&bytes).await.is_err() {
            return;
        }
    }
}

/// Raw message channels to the browser: its debugging pipe (Unix) or local
/// WebSocket (Windows). The second channel closes when the browser goes away.
async fn browser_link(
    transport: launch::Transport,
) -> Result<(mpsc::UnboundedSender<String>, mpsc::UnboundedReceiver<String>)> {
    let (to_tx, to_rx) = mpsc::unbounded_channel::<String>();
    let (from_tx, from_rx) = mpsc::unbounded_channel::<String>();
    match transport {
        #[cfg(unix)]
        launch::Transport::Pipe(w, r) => {
            tokio::spawn(write_messages(w, to_rx));
            tokio::spawn(async move { read_messages(r, |t| from_tx.send(t.to_string()).is_ok()).await });
        }
        launch::Transport::WebSocket(url) => {
            use futures_util::{SinkExt, StreamExt};
            use tokio_tungstenite::tungstenite::Message;
            let (ws, _) = tokio_tungstenite::connect_async(&url)
                .await
                .map_err(|e| CoreError::backend(format!("cannot connect to the browser: {e}")))?;
            let (mut sink, mut stream) = ws.split();
            let mut to_rx = to_rx;
            tokio::spawn(async move {
                while let Some(msg) = to_rx.recv().await {
                    if sink.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
            });
            tokio::spawn(async move {
                while let Some(Ok(msg)) = stream.next().await {
                    if let Message::Text(t) = msg {
                        if from_tx.send(t.as_str().to_string()).is_err() {
                            break;
                        }
                    }
                }
            });
        }
    }
    Ok((to_tx, from_rx))
}

/// Runs the daemon until idle timeout, browser exit or shutdown request.
pub async fn run(options: DaemonOptions) -> Result<()> {
    let dir = state_dir();
    private_dir(&dir).map_err(|e| CoreError::backend(format!("cannot prepare {}: {e}", dir.display())))?;
    let Some(mut listener) = ipc::Listener::bind()? else {
        return Ok(()); // another daemon is running
    };

    let exe = options
        .executable
        .clone()
        .or_else(launch::discover_headless_shell)
        .or_else(launch::discover)
        .ok_or_else(|| CoreError::unsupported("no Chromium found"))?;
    let launched = launch::launch(&exe, true, options.no_sandbox, &[]).await?;
    let mut child = launched.child;
    let _profile = launched.profile;
    let (browser_tx, mut browser_rx) = browser_link(launched.transport).await?;

    // Clients must present this token, readable only by this user.
    let token = new_token()?;
    let token_path = state_file("token");
    write_private(&token_path, &token).map_err(|e| CoreError::backend(format!("cannot write token: {e}")))?;

    let daemon = Daemon {
        state: Arc::new(Mutex::new(State { last_activity: Some(Instant::now()), ..State::default() })),
        browser: browser_tx,
        shutdown: Arc::new(tokio::sync::Notify::new()),
        started: Instant::now(),
        version: Arc::default(),
    };
    let browser_gone = Arc::new(tokio::sync::Notify::new());
    {
        let d = daemon.clone();
        let gone = browser_gone.clone();
        tokio::spawn(async move {
            while let Some(text) = browser_rx.recv().await {
                d.on_browser_message(&text);
            }
            gone.notify_one();
        });
    }
    daemon.refill_spare();
    {
        let d = daemon.clone();
        tokio::spawn(async move {
            if let Ok(v) = d.internal("Browser.getVersion", json!({})).await {
                *d.version.lock().unwrap() = v["product"].as_str().unwrap_or_default().to_string();
            }
        });
    }

    let mut idle = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok(Some((r, w))) = accepted else { continue };
                let d = daemon.clone();
                let token = token.clone();
                tokio::spawn(async move {
                    let (tx, rx) = mpsc::unbounded_channel::<String>();
                    tokio::spawn(write_messages(w, rx));
                    // Not registered as a client until it presents the token.
                    let mut client: Option<u64> = None;
                    read_messages(r, |text| match client {
                        Some(c) => {
                            d.on_client_message(c, text);
                            true
                        }
                        None => {
                            let msg: Value = serde_json::from_str(text).unwrap_or_default();
                            let ok = msg["method"] == "SwiftBrowse.hello" && msg["params"]["token"].as_str() == Some(token.as_str());
                            if !ok {
                                return false;
                            }
                            let mut st = d.state.lock().unwrap();
                            st.next_client += 1;
                            let c = st.next_client;
                            st.clients.insert(c, tx.clone());
                            st.last_activity = Some(Instant::now());
                            drop(st);
                            let _ = tx.send(json!({ "id": msg["id"], "result": {} }).to_string());
                            client = Some(c);
                            true
                        }
                    })
                    .await;
                    if let Some(c) = client {
                        d.disconnected(c).await;
                    }
                });
            }
            _ = idle.tick() => {
                let st = daemon.state.lock().unwrap();
                let idle_for = st.last_activity.map(|t| t.elapsed()).unwrap_or_default();
                if st.clients.is_empty() && idle_for >= options.idle_timeout {
                    break;
                }
            }
            _ = daemon.shutdown.notified() => break,
            _ = browser_gone.notified() => break,
        }
    }
    listener.remove();
    let _ = std::fs::remove_file(&token_path);
    let _ = tokio::time::timeout(Duration::from_secs(2), daemon.internal("Browser.close", json!({}))).await;
    if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
        let _ = child.kill().await;
    }
    Ok(())
}
