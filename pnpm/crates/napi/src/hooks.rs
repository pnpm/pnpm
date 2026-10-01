//! JS-backed `readPackage` hook.
//!
//! Bit transforms every dependency manifest during resolution (it strips
//! `@teambit/legacy` / `@teambit/harmony` and reshapes workspace-package deps).
//! pacquet applies such transforms through the [`PnpmfileHooks`] trait, which
//! the CLI loads from a `.pnpmfile.cjs`. [`JsReadPackageHook`] instead adapts a
//! host-supplied JS callback: `read_package` forwards each manifest to the
//! callback over a [`ThreadsafeFunction`] and awaits the transformed result.
//!
//! The callback receives `(manifest, resolvedDir?)` — `resolvedDir` is the
//! lockfile-root-relative directory when the manifest came from a directory
//! resolution (an injected workspace project or a `file:` dependency), so the
//! host can substitute a workspace project's raw manifest for its dependency
//! instances. The `.pnpmfile.cjs` bridge does not receive it, matching pnpm's
//! pnpmfile contract.
//!
//! The callback must be **synchronous** (return the manifest, not a promise) —
//! [`ThreadsafeFunction::call_async`] resolves the JS return value directly and
//! does not await a returned promise. Bit's composed `readPackage` hook is
//! synchronous.
//!
//! Only `read_package` is bridged; `after_all_resolved` returns
//! [`serde_json::Value::Null`] (pacquet's "no hook, keep the lockfile
//! unchanged" signal) and the remaining hooks are inert.
//!
//! The engine cannot fingerprint a JS callback, so both adapters report
//! themselves as an untracked `readPackage` hook, which makes every install
//! resolve again. [`ChecksummedHooks`] lets the host vouch for its hook with
//! a checksum instead.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use napi::{Status, bindgen_prelude::FnArgs, threadsafe_function::ThreadsafeFunction};
use pnpm_hooks::{
    CustomFetcher, CustomResolver, HookContext, HookError, PnpmfileHooks, PreResolutionHookContext,
    PreResolutionHookLogger, ReadPackageResult,
};
use serde_json::Value;

/// A synchronous JS `(manifest, resolvedDir?) => manifest` callback.
/// `CalleeHandled = false` (no leading error arg); the [`FnArgs`] wrapper
/// spreads the tuple into two JS arguments (a bare tuple would serialize into
/// a single JSON array) and the JS return value is deserialized back to a
/// manifest.
pub type HookSink = ThreadsafeFunction<
    FnArgs<(Value, Option<String>)>,
    Value,
    FnArgs<(Value, Option<String>)>,
    Status,
    false,
>;

/// A synchronous JS `(manifests, resolvedDirs) => manifests` batch callback
/// (parallel arrays; `resolvedDirs[i]` is `null` for a non-directory
/// resolution). The `@pnpm/napi` wrapper synthesizes it from the consumer's
/// per-manifest hook, so one event-loop traversal serves a whole batch —
/// per-call threadsafe dispatch caps out at roughly one call per loop tick,
/// which serialized a large install's resolution behind tens of thousands of
/// ticks.
pub type BatchHookSink = ThreadsafeFunction<
    FnArgs<(Vec<Value>, Vec<Option<String>>)>,
    Vec<Value>,
    FnArgs<(Vec<Value>, Vec<Option<String>>)>,
    Status,
    false,
>;

/// [`PnpmfileHooks`] implementation that runs `readPackage` through a JS
/// callback.
pub struct JsReadPackageHook {
    read_package: HookSink,
}

impl JsReadPackageHook {
    pub fn new(read_package: HookSink) -> Self {
        JsReadPackageHook { read_package }
    }
}

#[async_trait]
impl PnpmfileHooks for JsReadPackageHook {
    async fn read_package(
        &self,
        pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        match self.read_package.call_async(FnArgs::from((pkg, ctx.dir))).await {
            Ok(transformed) => Ok(Arc::new(transformed)),
            Err(error) => Err(HookError::Execution {
                pnpmfile: "<napi readPackage>".to_string(),
                message: error.to_string(),
            }),
        }
    }

    async fn after_all_resolved(
        &self,
        _lockfile: Value,
        _ctx: HookContext,
    ) -> Result<Value, HookError> {
        // Null signals "no afterAllResolved hook" — the caller keeps the
        // resolved lockfile unchanged.
        Ok(Value::Null)
    }

    async fn pre_resolution(
        &self,
        _ctx: PreResolutionHookContext,
        _logger: PreResolutionHookLogger,
    ) {
    }

    async fn filter_log(&self, _log: Value, _ctx: HookContext) -> bool {
        true
    }

    async fn has_read_package(&self) -> Result<bool, HookError> {
        Ok(true)
    }

    async fn untracked_read_package_hook(&self) -> Result<Option<bool>, HookError> {
        Ok(Some(true))
    }
}

/// Upper bound on manifests per batched JS call, keeping one call's
/// payload (and the JS-side synchronous map over it) bounded.
const MAX_HOOK_BATCH: usize = 256;

/// Capacity of the request queue feeding the batch driver. Bounded so a
/// stalled JS callback exerts backpressure on the resolver tasks instead
/// of letting them queue an unbounded number of full manifests in RAM.
const HOOK_QUEUE_CAPACITY: usize = MAX_HOOK_BATCH * 4;

/// Where one batched `readPackage` call's result is delivered.
type HookReply = tokio::sync::oneshot::Sender<Result<Value, String>>;

/// One queued `readPackage` request awaiting a slot in the next batch.
struct BatchHookRequest {
    manifest: Value,
    dir: Option<String>,
    reply: HookReply,
}

/// [`PnpmfileHooks`] implementation that runs `readPackage` through a
/// batching JS callback ([`BatchHookSink`]).
///
/// Requests from the resolver's concurrent walk are funneled through a
/// channel; a driver task drains whatever is queued (up to
/// [`MAX_HOOK_BATCH`]) into one threadsafe call. The driver is spawned
/// lazily on the first call so it lands on the same runtime the install
/// awaits on.
pub struct JsBatchedReadPackageHook {
    tx: tokio::sync::mpsc::Sender<BatchHookRequest>,
    driver_seed:
        std::sync::Mutex<Option<(tokio::sync::mpsc::Receiver<BatchHookRequest>, BatchHookSink)>>,
    driver_started: std::sync::Once,
}

impl JsBatchedReadPackageHook {
    pub fn new(read_package_batch: BatchHookSink) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(HOOK_QUEUE_CAPACITY);
        JsBatchedReadPackageHook {
            tx,
            driver_seed: std::sync::Mutex::new(Some((rx, read_package_batch))),
            driver_started: std::sync::Once::new(),
        }
    }

    fn ensure_driver(&self) {
        self.driver_started.call_once(|| {
            let (rx, sink) = self.driver_seed
                .lock()
                .expect("driver seed lock")
                .take()
                .expect("driver seed consumed exactly once");
            tokio::spawn(drive_hook_batches(rx, sink));
        });
    }
}

async fn drive_hook_batches(
    mut rx: tokio::sync::mpsc::Receiver<BatchHookRequest>,
    sink: BatchHookSink,
) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while batch.len() < MAX_HOOK_BATCH {
            let Ok(request) = rx.try_recv() else { break };
            batch.push(request);
        }
        run_hook_batch(&sink, batch).await;
    }
}

/// Call the hook once for the whole batch and hand each caller its own
/// result. Every request is answered, so a failing batch surfaces as an
/// error at each call site instead of leaving it waiting.
async fn run_hook_batch(sink: &BatchHookSink, batch: Vec<BatchHookRequest>) {
    let mut manifests = Vec::with_capacity(batch.len());
    let mut dirs = Vec::with_capacity(batch.len());
    let mut replies = Vec::with_capacity(batch.len());
    for request in batch {
        manifests.push(request.manifest);
        dirs.push(request.dir);
        replies.push(request.reply);
    }

    let results = match sink.call_async(FnArgs::from((manifests, dirs))).await {
        Ok(results) if results.len() == replies.len() => results,
        Ok(results) => {
            let message = format!(
                "batched readPackage hook returned {} manifests for {} inputs",
                results.len(),
                replies.len(),
            );
            return fail_replies(replies, &message);
        }
        Err(error) => return fail_replies(replies, &error.to_string()),
    };
    for (reply, result) in replies.into_iter().zip(results) {
        let _ = reply.send(Ok(result));
    }
}

fn fail_replies(replies: Vec<HookReply>, message: &str) {
    for reply in replies {
        let _ = reply.send(Err(message.to_string()));
    }
}

#[async_trait]
impl PnpmfileHooks for JsBatchedReadPackageHook {
    async fn read_package(
        &self,
        pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        self.ensure_driver();
        let (reply, response) = tokio::sync::oneshot::channel();
        let request = BatchHookRequest { manifest: pkg, dir: ctx.dir, reply };
        let execution_error = |message: String| HookError::Execution {
            pnpmfile: "<napi readPackage>".to_string(),
            message,
        };
        self.tx
            .send(request)
            .await
            .map_err(|_| execution_error("readPackage hook driver stopped".to_string()))?;
        match response.await {
            Ok(Ok(transformed)) => Ok(Arc::new(transformed)),
            Ok(Err(message)) => Err(execution_error(message)),
            Err(_) => Err(execution_error("readPackage hook driver dropped a reply".to_string())),
        }
    }

    async fn after_all_resolved(
        &self,
        _lockfile: Value,
        _ctx: HookContext,
    ) -> Result<Value, HookError> {
        // Null signals "no afterAllResolved hook" — the caller keeps the
        // resolved lockfile unchanged.
        Ok(Value::Null)
    }

    async fn pre_resolution(
        &self,
        _ctx: PreResolutionHookContext,
        _logger: PreResolutionHookLogger,
    ) {
    }

    async fn filter_log(&self, _log: Value, _ctx: HookContext) -> bool {
        true
    }

    async fn has_read_package(&self) -> Result<bool, HookError> {
        Ok(true)
    }

    async fn untracked_read_package_hook(&self) -> Result<Option<bool>, HookError> {
        Ok(Some(true))
    }
}

/// Records a host-supplied checksum as the `pnpmfileChecksum` of the hooks
/// it wraps, the way a `.pnpmfile.cjs` is recorded by the hash of its
/// content. The wrapped `readPackage` hook is therefore tracked: the
/// lockfile is reused while the checksum stays the same and resolved again
/// when it changes.
pub struct ChecksummedHooks {
    hooks: Arc<dyn PnpmfileHooks>,
    checksum: String,
}

impl ChecksummedHooks {
    /// Wraps `hooks` when the host gave a checksum. Without one, `hooks` stay
    /// untracked.
    pub fn wrap(
        hooks: Option<Arc<dyn PnpmfileHooks>>,
        checksum: Option<&str>,
    ) -> Option<Arc<dyn PnpmfileHooks>> {
        match (hooks, checksum) {
            (Some(hooks), Some(checksum)) => {
                Some(Arc::new(ChecksummedHooks { hooks, checksum: checksum.to_string() }))
            }
            (hooks, _) => hooks,
        }
    }
}

#[async_trait]
impl PnpmfileHooks for ChecksummedHooks {
    async fn read_package(
        &self,
        pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        self.hooks.read_package(pkg, ctx).await
    }

    async fn after_all_resolved(
        &self,
        lockfile: Value,
        ctx: HookContext,
    ) -> Result<Value, HookError> {
        self.hooks.after_all_resolved(lockfile, ctx).await
    }

    async fn update_config(&self, config: Value, ctx: HookContext) -> Result<Value, HookError> {
        self.hooks.update_config(config, ctx).await
    }

    async fn before_packing(
        &self,
        manifest: Value,
        dir: &Path,
        ctx: HookContext,
    ) -> Result<Value, HookError> {
        self.hooks.before_packing(manifest, dir, ctx).await
    }

    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) {
        self.hooks.pre_resolution(ctx, logger).await;
    }

    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool {
        self.hooks.filter_log(log, ctx).await
    }

    async fn has_filter_log(&self) -> bool {
        self.hooks.has_filter_log().await
    }

    async fn has_read_package(&self) -> Result<bool, HookError> {
        self.hooks.has_read_package().await
    }

    async fn has_after_all_resolved(&self) -> Result<bool, HookError> {
        self.hooks.has_after_all_resolved().await
    }

    async fn has_pre_resolution(&self) -> Result<bool, HookError> {
        self.hooks.has_pre_resolution().await
    }

    async fn untracked_read_package_hook(&self) -> Result<Option<bool>, HookError> {
        Ok(None)
    }

    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        Some(self.checksum.clone())
    }

    async fn get_custom_resolvers(&self) -> Result<Vec<Arc<dyn CustomResolver>>, HookError> {
        self.hooks.get_custom_resolvers().await
    }

    async fn get_custom_fetchers(&self) -> Result<Vec<Arc<dyn CustomFetcher>>, HookError> {
        self.hooks.get_custom_fetchers().await
    }

    async fn get_finder_names(&self) -> Result<Vec<String>, HookError> {
        self.hooks.get_finder_names().await
    }

    async fn run_finder(&self, finder_name: &str, ctx: Value) -> Result<Value, HookError> {
        self.hooks.run_finder(finder_name, ctx).await
    }
}
