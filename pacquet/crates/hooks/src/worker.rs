//! A long-lived Node.js worker that loads a pnpmfile once and serves hook
//! invocations over a newline-delimited JSON protocol on stdin/stdout.
//!
//! Spawning a fresh `node` process per hook call is prohibitively expensive on
//! the resolution hot path, where `readPackage` runs once per resolved package.
//! The worker loads the pnpmfile a single time and answers many requests,
//! multiplexed by a monotonic request id so concurrent calls (the resolver
//! resolves dependencies in parallel) can be in flight at once.
//!
//! Protocol — one JSON object per line:
//! - request:  `{"id": N, "hook": "readPackage", "payload": <value>}`
//! - query:    `{"id": N, "query": "hasHooks"}`     (does the module export `hooks`?)
//! - log:      `{"id": N, "log": "message"}`        (a `context.log(...)` call)
//! - success:  `{"id": N, "ok": <value>}`
//! - failure:  `{"id": N, "err": "message"}`

use crate::HookError;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, oneshot},
    time::{Duration, timeout},
};

const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// Callback invoked for each `context.log(...)` a hook emits while it runs.
pub type LogFn = Arc<dyn Fn(String) + Send + Sync>;

/// Which optional methods one custom resolver in the pnpmfile's
/// `resolvers` array implements. Mirrors the optional methods of pnpm's
/// `CustomResolver` interface (`hooks/types/src/index.ts`).
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolverCapabilities {
    pub can_resolve: bool,
    pub resolve: bool,
    pub should_refresh_resolution: bool,
}

struct Pending {
    log: LogFn,
    done: oneshot::Sender<Result<Value, String>>,
}

/// Pending requests keyed by id. A `std` mutex (never held across an
/// await) so [`PendingEntryGuard::drop`] can clean up synchronously
/// when a caller's future is cancelled.
type PendingMap = Arc<StdMutex<HashMap<u64, Pending>>>;

/// Removes the pending entry on drop. Covers every exit from
/// [`NodeWorker::request`] — send failure, timeout, and cancellation of
/// the caller's future — so a request the worker never answers cannot
/// leak its entry. Removal is idempotent: a delivered reply has already
/// removed the entry in [`dispatch_line`].
struct PendingEntryGuard {
    pending: PendingMap,
    id: u64,
}

impl Drop for PendingEntryGuard {
    fn drop(&mut self) {
        self.pending.lock().unwrap().remove(&self.id);
    }
}

/// A handle to a running Node worker process loaded with one pnpmfile.
pub struct NodeWorker {
    pnpmfile: String,
    stdin: Mutex<ChildStdin>,
    pending: PendingMap,
    next_id: AtomicU64,
    /// Kept so the child is killed when the worker is dropped (`kill_on_drop`).
    _child: Child,
}

impl NodeWorker {
    /// Spawn the worker for `file` and start reading its responses.
    pub async fn spawn(file: &Path) -> Result<Arc<NodeWorker>, HookError> {
        let pnpmfile = file.to_string_lossy().into_owned();
        let exec_err =
            |message: String| HookError::Execution { pnpmfile: pnpmfile.clone(), message };

        let file_escaped =
            serde_json::to_string(&pnpmfile).map_err(|err| exec_err(err.to_string()))?;
        let is_mjs = pnpmfile.ends_with(".mjs");
        let runner = build_runner(is_mjs, &file_escaped);

        // The runner itself is always CommonJS so it can `require('node:readline')`;
        // an `.mjs` pnpmfile is loaded through dynamic `import()`, which works
        // from CommonJS.
        let mut child = Command::new("node")
            .arg("--input-type")
            .arg("commonjs")
            .arg("-e")
            .arg(&runner)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| exec_err(err.to_string()))?;

        let stdin = child.stdin.take().expect("worker stdin is piped");
        let stdout = child.stdout.take().expect("worker stdout is piped");

        let pending: PendingMap = Arc::new(StdMutex::new(HashMap::new()));
        let pending_reader = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                dispatch_line(&pending_reader, &line);
            }
            // The worker exited: fail every still-pending request so callers
            // don't hang waiting for a response that will never arrive.
            for (_, pending) in pending_reader.lock().unwrap().drain() {
                let _ = pending.done.send(Err("pnpmfile worker exited".to_string()));
            }
        });

        Ok(Arc::new(NodeWorker {
            pnpmfile,
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicU64::new(0),
            _child: child,
        }))
    }

    fn exec_err(&self, message: impl Into<String>) -> HookError {
        HookError::Execution { pnpmfile: self.pnpmfile.clone(), message: message.into() }
    }

    /// Run `hook` with `payload`, forwarding any `context.log(...)` to `log`.
    pub async fn call(&self, hook: &str, payload: Value, log: LogFn) -> Result<Value, HookError> {
        self.request(hook, serde_json::json!({ "hook": hook, "payload": payload }), log).await
    }

    /// Whether the loaded pnpmfile exports a `hooks` object. Mirrors
    /// pnpm's `entry.hooks != null` gate for `pnpmfileChecksum`.
    pub async fn has_hooks(&self) -> bool {
        self.request("hasHooks", serde_json::json!({ "query": "hasHooks" }), Arc::new(|_| {}))
            .await
            .ok()
            .as_ref()
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Call `method` on the custom resolver at `index` in the pnpmfile's
    /// `resolvers` array, forwarding any `context.log(...)` to `log`.
    pub async fn call_resolver(
        &self,
        index: usize,
        method: &str,
        payload: Value,
        log: LogFn,
    ) -> Result<Value, HookError> {
        self.request(
            method,
            serde_json::json!({
                "target": "resolver",
                "index": index,
                "method": method,
                "payload": payload,
            }),
            log,
        )
        .await
    }

    /// Get the capabilities of every custom resolver exported by the
    /// pnpmfile's `resolvers` array, in array order. Lets callers skip
    /// per-dependency IPC round trips for methods a resolver does not
    /// implement, mirroring pnpm's optional-method checks
    /// (`if (!customResolver.canResolve || !customResolver.resolve) continue`).
    pub async fn get_resolver_capabilities(&self) -> Result<Vec<ResolverCapabilities>, HookError> {
        let value = self
            .request("resolvers", serde_json::json!({ "target": "resolvers" }), Arc::new(|_| {}))
            .await?;
        serde_json::from_value(value).map_err(|err| self.exec_err(err.to_string()))
    }

    /// Send one request `body` (an object the worker dispatches on) and
    /// await its reply, stamping in the request id and routing any
    /// `context.log(...)` lines to `log`. `label` names the request in a
    /// timeout error.
    async fn request(&self, label: &str, mut body: Value, log: LogFn) -> Result<Value, HookError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (done, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, Pending { log, done });
        let _pending_guard = PendingEntryGuard { pending: Arc::clone(&self.pending), id };

        body["id"] = serde_json::json!(id);
        let mut line =
            serde_json::to_string(&body).map_err(|err| self.exec_err(err.to_string()))?;
        line.push('\n');
        {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(line.as_bytes()).await.map_err(|err| self.exec_err(err.to_string()))?;
            stdin.flush().await.map_err(|err| self.exec_err(err.to_string()))?;
        }

        match timeout(HOOK_TIMEOUT, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(message))) => Err(self.exec_err(message)),
            Ok(Err(_)) => Err(self.exec_err("pnpmfile worker dropped the response")),
            Err(_) => Err(HookError::Timeout(label.to_string(), HOOK_TIMEOUT.as_secs())),
        }
    }
}

/// Route one line from the worker to its pending request: forward `log` lines
/// to the call's logger (the entry stays until the result arrives) and resolve
/// the call on `ok`/`err`.
fn dispatch_line(pending: &PendingMap, line: &str) {
    let Ok(message) = serde_json::from_str::<Value>(line) else { return };
    let Some(id) = message.get("id").and_then(Value::as_u64) else { return };

    if let Some(log) = message.get("log").and_then(Value::as_str) {
        let log_fn = pending.lock().unwrap().get(&id).map(|entry| Arc::clone(&entry.log));
        if let Some(log_fn) = log_fn {
            log_fn(log.to_string());
        }
        return;
    }

    let Some(entry) = pending.lock().unwrap().remove(&id) else { return };
    let result = match message.get("err").and_then(Value::as_str) {
        Some(err) => Err(err.to_string()),
        None => Ok(message.get("ok").cloned().unwrap_or(Value::Null)),
    };
    let _ = entry.done.send(result);
}

/// Build the worker's Node script. `file_escaped` is the JSON-encoded pnpmfile
/// path; the worker loads it once and replays the `readPackage` validation and
/// normalization that [`crate::node_runtime`] documents.
fn build_runner(is_mjs: bool, file_escaped: &str) -> String {
    let load = if is_mjs {
        format!("mod = await import({file_escaped});")
    } else {
        format!("mod = require({file_escaped});")
    };
    format!(
        r#"const readline = require('node:readline');
let mod = null;
let loadErr = null;
async function ensureLoaded() {{
  if (mod !== null || loadErr !== null) return;
  try {{ {load} }} catch (err) {{ loadErr = err && err.stack ? err.stack : String(err); }}
}}
const rl = readline.createInterface({{ input: process.stdin }});
rl.on('line', (line) => {{ handle(line); }});
async function handle(line) {{
  let req;
  try {{ req = JSON.parse(line); }} catch {{ return; }}
  const id = req.id;
  const send = (obj) => process.stdout.write(JSON.stringify(Object.assign({{ id }}, obj)) + '\n');
  await ensureLoaded();
  if (loadErr !== null) {{ send({{ err: loadErr }}); return; }}
  if (req.query === 'hasHooks') {{ send({{ ok: mod != null && mod.hooks != null }}); return; }}
  try {{
    const fn = mod && mod.hooks && mod.hooks[req.hook];
    const context = {{ log: (m) => send({{ log: String(m) }}) }};
    if (req.target === 'resolvers') {{
      const resolvers = mod && mod.resolvers;
      send({{ ok: (Array.isArray(resolvers) ? resolvers.map((resolver) => ({{
        canResolve: typeof resolver.canResolve === 'function',
        resolve: typeof resolver.resolve === 'function',
        shouldRefreshResolution: typeof resolver.shouldRefreshResolution === 'function',
      }})) : []) }});
      return;
    }}
    if (req.target === 'resolver') {{
      const resolvers = mod && mod.resolvers;
      const resolver = Array.isArray(resolvers) ? resolvers[req.index] : null;
      if (!resolver || typeof resolver[req.method] !== 'function') {{
        send({{ ok: null }});
        return;
      }}
      const args = req.payload || [];
      const res = await resolver[req.method](...args);
      send({{ ok: res === undefined ? null : res }});
      return;
    }}
    if (req.hook === 'readPackage') {{
      const pkg = req.payload;
      if (typeof fn !== 'function') {{ send({{ ok: pkg }}); return; }}
      pkg.dependencies = pkg.dependencies ?? {{}};
      pkg.devDependencies = pkg.devDependencies ?? {{}};
      pkg.optionalDependencies = pkg.optionalDependencies ?? {{}};
      pkg.peerDependencies = pkg.peerDependencies ?? {{}};
      const newPkg = await fn(pkg, context);
      if (!newPkg) {{
        throw new Error("readPackage hook did not return a package manifest object. Hook imported via " + {file_escaped});
      }}
      for (const dep of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]) {{
        const v = newPkg[dep];
        if (v != null && (typeof v !== "object" || Array.isArray(v))) {{
          throw new Error("readPackage hook returned package manifest object's property '" + dep + "' must be an object. Hook imported via " + {file_escaped});
        }}
      }}
      send({{ ok: newPkg }});
    }} else if (req.hook === 'filterLog') {{
      const res = (typeof fn === 'function') ? await fn(req.payload, context) : true;
      send({{ ok: res }});
    }} else {{
      const res = (typeof fn === 'function') ? await fn(req.payload, context) : null;
      send({{ ok: res === undefined ? null : res }});
    }}
  }} catch (err) {{
    send({{ err: err && err.stack ? err.stack : String(err) }});
  }}
}}
"#,
    )
}

#[cfg(test)]
mod tests;
