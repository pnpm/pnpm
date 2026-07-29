//! `readPackage` hook implementations: JS-backed bridges plus the pure-Rust
//! [`IgnoredDependenciesHook`] layer.
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

use std::sync::Arc;

use async_trait::async_trait;
use napi::{Status, bindgen_prelude::FnArgs, threadsafe_function::ThreadsafeFunction};
use pacquet_hooks::{
    HookContext, HookError, PnpmfileHooks, PreResolutionHookContext, PreResolutionHookLogger,
    ReadPackageResult,
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
}

/// [`PnpmfileHooks`] layer that removes the `ignoredDependencies` install
/// option's package names from every resolved manifest — from
/// `dependencies` (unless the range is a `link:`, an explicit local
/// reference that must survive) and from `peerDependencies` — after the
/// optional inner JS hook has run. With no inner hook this makes the
/// engine's whole manifest-transform chain pure Rust: no JS round trips.
pub struct IgnoredDependenciesHook {
    inner: Option<Arc<dyn PnpmfileHooks>>,
    ignored_dependencies: Vec<String>,
}

impl IgnoredDependenciesHook {
    pub fn new(inner: Option<Arc<dyn PnpmfileHooks>>, ignored_dependencies: Vec<String>) -> Self {
        IgnoredDependenciesHook { inner, ignored_dependencies }
    }

    /// `true` when the range keeps the dependency exempt from removal.
    fn keeps_entry(range: &Value) -> bool {
        range.as_str().is_some_and(|range| range.starts_with("link:"))
    }

    fn removes_anything_from(&self, manifest: &Value) -> bool {
        self.ignored_dependencies.iter().any(|name| {
            manifest
                .get("dependencies")
                .and_then(|deps| deps.get(name))
                .is_some_and(|range| !Self::keeps_entry(range))
                || manifest.get("peerDependencies").and_then(|deps| deps.get(name)).is_some()
        })
    }

    /// Strip the ignored names, sharing the input `Arc` when nothing matches.
    fn strip(&self, manifest: ReadPackageResult) -> ReadPackageResult {
        if !self.removes_anything_from(&manifest) {
            return manifest;
        }
        let mut owned = (*manifest).clone();
        for name in &self.ignored_dependencies {
            if let Some(deps) = owned.get_mut("dependencies").and_then(Value::as_object_mut)
                && deps.get(name).is_some_and(|range| !Self::keeps_entry(range))
            {
                deps.remove(name);
            }
            if let Some(peers) = owned.get_mut("peerDependencies").and_then(Value::as_object_mut) {
                peers.remove(name);
            }
        }
        Arc::new(owned)
    }
}

#[async_trait]
impl PnpmfileHooks for IgnoredDependenciesHook {
    async fn read_package(
        &self,
        pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        let manifest = match &self.inner {
            Some(inner) => inner.read_package(pkg, ctx).await?,
            None => Arc::new(pkg),
        };
        Ok(self.strip(manifest))
    }

    async fn after_all_resolved(
        &self,
        lockfile: Value,
        ctx: HookContext,
    ) -> Result<Value, HookError> {
        match &self.inner {
            Some(inner) => inner.after_all_resolved(lockfile, ctx).await,
            // Null signals "no afterAllResolved hook" — the caller keeps the
            // resolved lockfile unchanged.
            None => Ok(Value::Null),
        }
    }

    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) {
        if let Some(inner) = &self.inner {
            inner.pre_resolution(ctx, logger).await;
        }
    }

    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool {
        match &self.inner {
            Some(inner) => inner.filter_log(log, ctx).await,
            None => true,
        }
    }
}

/// Upper bound on manifests per batched JS call, keeping one call's
/// payload (and the JS-side synchronous map over it) bounded.
const MAX_HOOK_BATCH: usize = 256;

/// One queued `readPackage` request awaiting a slot in the next batch.
struct BatchHookRequest {
    manifest: Value,
    dir: Option<String>,
    reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
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
    tx: tokio::sync::mpsc::UnboundedSender<BatchHookRequest>,
    driver_seed: std::sync::Mutex<
        Option<(tokio::sync::mpsc::UnboundedReceiver<BatchHookRequest>, BatchHookSink)>,
    >,
    driver_started: std::sync::Once,
}

impl JsBatchedReadPackageHook {
    pub fn new(read_package_batch: BatchHookSink) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        JsBatchedReadPackageHook {
            tx,
            driver_seed: std::sync::Mutex::new(Some((rx, read_package_batch))),
            driver_started: std::sync::Once::new(),
        }
    }

    fn ensure_driver(&self) {
        self.driver_started.call_once(|| {
            let (rx, sink) = self
                .driver_seed
                .lock()
                .expect("driver seed lock")
                .take()
                .expect("driver seed consumed exactly once");
            tokio::spawn(drive_hook_batches(rx, sink));
        });
    }
}

async fn drive_hook_batches(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<BatchHookRequest>,
    sink: BatchHookSink,
) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while batch.len() < MAX_HOOK_BATCH {
            match rx.try_recv() {
                Ok(request) => batch.push(request),
                Err(_) => break,
            }
        }
        let mut manifests = Vec::with_capacity(batch.len());
        let mut dirs = Vec::with_capacity(batch.len());
        let mut replies = Vec::with_capacity(batch.len());
        for request in batch {
            manifests.push(request.manifest);
            dirs.push(request.dir);
            replies.push(request.reply);
        }
        match sink.call_async(FnArgs::from((manifests, dirs))).await {
            Ok(results) if results.len() == replies.len() => {
                for (reply, result) in replies.into_iter().zip(results) {
                    let _ = reply.send(Ok(result));
                }
            }
            Ok(results) => {
                let message = format!(
                    "batched readPackage hook returned {} manifests for {} inputs",
                    results.len(),
                    replies.len(),
                );
                for reply in replies {
                    let _ = reply.send(Err(message.clone()));
                }
            }
            Err(error) => {
                let message = error.to_string();
                for reply in replies {
                    let _ = reply.send(Err(message.clone()));
                }
            }
        }
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
}

#[cfg(test)]
mod tests;
