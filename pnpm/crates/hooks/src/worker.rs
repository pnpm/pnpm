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

use crate::{FetcherCallbackSender, HookError};
use protocol::{spawn_stdout_reader, write_worker_line};
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
    process::{Child, ChildStdin, Command},
    sync::{Mutex, Semaphore, oneshot},
    time::{Duration, timeout},
};

const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// Floor for a `fetch` that was handed native tarball callbacks. Such a
/// call is not waiting on the pnpmfile's own code — it is waiting on the
/// downloads and extractions the installer runs on its behalf, which a
/// hook-sized budget would abort mid-archive.
const CALLBACK_FETCH_TIMEOUT: Duration = Duration::from_mins(5);

/// How many requests may be in flight to one worker at a time.
///
/// Callers fan out far wider: the resolver runs `readPackage` once per
/// resolved package, all at once, which on a large install is thousands
/// of requests. One worker is one Node process, so writing them all at
/// once buys no throughput — it only makes each request queue behind the
/// rest while its [`HOOK_TIMEOUT`] runs, until a batch of fast hooks
/// fails the install on a timeout none of them caused.
const MAX_IN_FLIGHT_REQUESTS: usize = 16;

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

/// Which optional methods one custom fetcher in the pnpmfile's
/// `fetchers` array implements.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetcherCapabilities {
    pub can_fetch: bool,
    pub fetch: bool,
}

struct Pending {
    log: LogFn,
    done: oneshot::Sender<Result<Value, String>>,
    callbacks: Option<FetcherCallbackSender>,
}

/// Pending requests keyed by id. A `std` mutex (never held across an
/// await) so [`PendingEntryGuard::drop`] can clean up synchronously
/// when a caller's future is cancelled.
type PendingMap = Arc<StdMutex<HashMap<u64, Pending>>>;

/// Removes the pending entry on drop. Covers every exit from
/// [`NodeWorker::request`] — send failure, timeout, and cancellation of
/// the caller's future — so a request the worker never answers cannot
/// leak its entry. Removal is idempotent: a delivered reply has already
/// removed the entry while dispatching the response.
struct PendingEntryGuard {
    pending: PendingMap,
    id: u64,
}

impl Drop for PendingEntryGuard {
    fn drop(&mut self) {
        self.pending
            .lock()
            .unwrap()
            .remove(&self.id);
    }
}

/// A handle to a running Node worker process loaded with one pnpmfile.
pub struct NodeWorker {
    pnpmfile: String,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: PendingMap,
    next_id: AtomicU64,
    in_flight: Semaphore,
    /// How long a request may take from the moment it is written to the
    /// worker. [`HOOK_TIMEOUT`] outside tests.
    request_timeout: Duration,
    /// Kept so the child is killed when the worker is dropped (`kill_on_drop`).
    _child: Child,
}

impl NodeWorker {
    /// Spawn the worker for `file` and start reading its responses.
    pub async fn spawn(file: &Path) -> Result<Arc<NodeWorker>, HookError> {
        NodeWorker::spawn_with_request_timeout(file, HOOK_TIMEOUT).await
    }

    async fn spawn_with_request_timeout(
        file: &Path,
        request_timeout: Duration,
    ) -> Result<Arc<NodeWorker>, HookError> {
        let pnpmfile = file.to_string_lossy().into_owned();
        let exec_err = |message: String| HookError::Execution {
            pnpmfile: pnpmfile.clone(),
            message,
        };

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

        let stdin = Arc::new(Mutex::new(
            child.stdin.take().expect("worker stdin is piped"),
        ));
        let stdout = child.stdout.take().expect("worker stdout is piped");

        let pending: PendingMap = Arc::new(StdMutex::new(HashMap::new()));
        spawn_stdout_reader(stdout, Arc::clone(&pending), Arc::clone(&stdin));

        Ok(Arc::new(NodeWorker {
            pnpmfile,
            stdin,
            pending,
            next_id: AtomicU64::new(0),
            in_flight: Semaphore::new(MAX_IN_FLIGHT_REQUESTS),
            request_timeout,
            _child: child,
        }))
    }

    fn exec_err(&self, message: impl Into<String>) -> HookError {
        HookError::Execution {
            pnpmfile: self.pnpmfile.clone(),
            message: message.into(),
        }
    }

    /// Run `hook` with `payload`, forwarding any `context.log(...)` to `log`.
    pub async fn call(&self, hook: &str, payload: Value, log: LogFn) -> Result<Value, HookError> {
        self.request(
            hook,
            serde_json::json!({ "hook": hook, "payload": payload }),
            log,
        )
        .await
    }

    /// Call the `beforePacking` hook with `(manifest, dir, context)`.
    /// Returns the hook's result, or `manifest` unchanged when the
    /// pnpmfile exports no `beforePacking` (mirroring pnpm's
    /// `await hook(pkg, dir) ?? publishManifest`).
    pub async fn call_before_packing(
        &self,
        manifest: Value,
        dir: &str,
        log: LogFn,
    ) -> Result<Value, HookError> {
        self.request(
            "beforePacking",
            serde_json::json!({ "hook": "beforePacking", "payload": manifest, "dir": dir }),
            log,
        )
        .await
    }

    /// Whether the loaded pnpmfile exports a `hooks` object. Mirrors
    /// pnpm's `entry.hooks != null` gate for `pnpmfileChecksum`.
    pub async fn has_hooks(&self) -> bool {
        self
            .request(
                "hasHooks",
                serde_json::json!({ "query": "hasHooks" }),
                Arc::new(|_| {}),
            )
            .await
            .ok()
            .as_ref()
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Whether the loaded pnpmfile exports a callable `filterLog` hook.
    pub async fn has_filter_log(&self) -> bool {
        self
            .request(
                "hasFilterLog",
                serde_json::json!({ "query": "hasFilterLog" }),
                Arc::new(|_| {}),
            )
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
        let value = self.request(
            "resolvers",
            serde_json::json!({ "target": "resolvers" }),
            Arc::new(|_| {}),
        )
        .await?;
        serde_json::from_value(value).map_err(|err| self.exec_err(err.to_string()))
    }

    /// Call `method` on the custom fetcher at `index` in the pnpmfile's
    /// `fetchers` array, forwarding any `context.log(...)` to `log`.
    pub async fn call_fetcher(
        &self,
        index: usize,
        method: &str,
        payload: Value,
        log: LogFn,
        callbacks: Option<FetcherCallbackSender>,
    ) -> Result<Value, HookError> {
        self.request_with_callbacks(
            method,
            serde_json::json!({
                "target": "fetcher",
                "index": index,
                "method": method,
                "payload": payload,
                "callbacks": callbacks.is_some(),
            }),
            log,
            callbacks,
        )
        .await
    }

    /// Get the capabilities of every custom fetcher exported by the
    /// pnpmfile's `fetchers` array, in array order.
    pub async fn get_fetcher_capabilities(&self) -> Result<Vec<FetcherCapabilities>, HookError> {
        let value = self.request(
            "fetchers",
            serde_json::json!({ "target": "fetchers" }),
            Arc::new(|_| {}),
        )
        .await?;
        serde_json::from_value(value).map_err(|err| self.exec_err(err.to_string()))
    }

    pub async fn get_finder_names(&self) -> Result<Vec<String>, HookError> {
        let value = self.request(
            "finders",
            serde_json::json!({ "target": "finders" }),
            Arc::new(|_| {}),
        )
        .await?;
        serde_json::from_value(value).map_err(|err| self.exec_err(err.to_string()))
    }

    /// `ctx.manifest` is passed pre-read because a JavaScript callback
    /// cannot call back over the pipe: the runner wraps it as the
    /// `readManifest()` the finder contract expects.
    pub async fn call_finder(&self, name: &str, ctx: Value) -> Result<Value, HookError> {
        self.request(
            "finder",
            serde_json::json!({ "target": "finder", "name": name, "ctx": ctx }),
            Arc::new(|_| {}),
        )
        .await
    }

    /// Send one request `body` (an object the worker dispatches on) and
    /// await its reply, stamping in the request id and routing any
    /// `context.log(...)` lines to `log`. `label` names the request in a
    /// timeout error.
    ///
    /// Waits for one of the worker's [`MAX_IN_FLIGHT_REQUESTS`] slots
    /// first, so a request's timeout window covers only the time the
    /// worker had it, not the time its caller's fan-out spent queued.
    async fn request(&self, label: &str, body: Value, log: LogFn) -> Result<Value, HookError> {
        self.request_with_callbacks(label, body, log, None).await
    }

    async fn request_with_callbacks(
        &self,
        label: &str,
        mut body: Value,
        log: LogFn,
        callbacks: Option<FetcherCallbackSender>,
    ) -> Result<Value, HookError> {
        let _in_flight = self.in_flight.acquire().await.expect("the semaphore is never closed");

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (done, rx) = oneshot::channel();
        let callback_timeout = label == "fetch" && callbacks.is_some();
        self.pending
            .lock()
            .unwrap()
            .insert(
                id,
                Pending {
                    log,
                    done,
                    callbacks,
                },
            );
        let _pending_guard = PendingEntryGuard {
            pending: Arc::clone(&self.pending),
            id,
        };

        body["id"] = serde_json::json!(id);
        write_worker_line(&self.stdin, &body).await
            .map_err(|error| self.exec_err(error.to_string()))?;

        let request_timeout = if callback_timeout {
            self.request_timeout.max(CALLBACK_FETCH_TIMEOUT)
        } else {
            self.request_timeout
        };
        match timeout(request_timeout, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(message))) => Err(self.exec_err(message)),
            Ok(Err(_)) => Err(self.exec_err("pnpmfile worker dropped the response")),
            Err(_) => Err(HookError::Timeout(
                label.to_string(),
                request_timeout.as_secs(),
            )),
        }
    }
}

/// Build the worker's Node script. `file_escaped` is the JSON-encoded pnpmfile
/// path; the worker loads it once and replays the `readPackage` validation and
/// normalization that [`crate::node_runtime`] documents.
fn build_runner(is_mjs: bool, file_escaped: &str) -> String {
    format!(
        "const pnpmfilePath = {file_escaped};\nconst pnpmfileIsMjs = {is_mjs};\n{}",
        include_str!("worker.cjs"),
    )
}

#[cfg(test)]
mod tests;

mod protocol;
