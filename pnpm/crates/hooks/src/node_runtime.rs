use crate::{HookError, worker::NodeWorker};
use async_trait::async_trait;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
    sync::OnceCell,
    time::{Duration, timeout},
};

/// Runs `.pnpmfile.{cjs,mjs}` hooks via Node.js.
///
/// `readPackage`, `afterAllResolved`, and `filterLog` are served by a
/// long-lived [`NodeWorker`] (spawned lazily, once per pnpmfile) so the
/// per-package `readPackage` calls on the resolution hot path don't each pay a
/// `node` startup. `preResolution` keeps a one-shot `node -e` invocation: it
/// runs once per install and needs an `info`/`warn` logger rather than the
/// worker's `context.log`.
pub struct NodeJsHooks {
    pub file: PathBuf,
    worker: OnceCell<Result<Arc<NodeWorker>, HookError>>,
}

const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

impl NodeJsHooks {
    #[must_use]
    pub fn new(file: PathBuf) -> Self {
        NodeJsHooks { file, worker: OnceCell::new() }
    }

    /// The worker process, spawned on first use and reused thereafter. A spawn
    /// failure is cached and surfaced to every hook call.
    async fn worker(&self) -> Result<Arc<NodeWorker>, HookError> {
        self.worker.get_or_init(|| NodeWorker::spawn(&self.file)).await.clone()
    }

    /// Runs a side-effecting hook (`preResolution`) in a one-shot `node`
    /// process, piping the JSON context on stdin and exposing an
    /// `info`/`warn` logger. Failures are reported through `logger.warn`
    /// rather than aborting, matching the hook's advisory role.
    async fn call_node_void(
        &self,
        func: &str,
        args: Value,
        logger: &crate::PreResolutionHookLogger,
    ) {
        let Ok(ctx_payload) = serde_json::to_string(&args) else { return };
        let Some((input_type, wrapper)) = hook_wrapper(&self.file.to_string_lossy(), func) else {
            return;
        };

        let Ok(mut child) = Command::new("node")
            .arg("--input-type")
            .arg(input_type)
            .arg("-e")
            .arg(&wrapper)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        else {
            (logger.warn)("pnpmfile hook failed to start".to_string());
            return;
        };

        let hook_result = drive_hook(&mut child, &ctx_payload, logger).await;

        let Ok((stderr_tail, Ok(status))) = hook_result else {
            (logger.warn)("pnpmfile hook timed out or failed to execute".to_string());
            return;
        };

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr_tail);
            (logger.warn)(format!("pnpmfile hook failed: {stderr}"));
        }
    }
}

/// The Node wrapper that loads the pnpmfile and calls `func` with the
/// context read from stdin, keyed by the module type Node must parse it as.
fn hook_wrapper(file_path: &str, func: &str) -> Option<(&'static str, String)> {
    let file_path_escaped = serde_json::to_string(file_path).ok()?;
    let (input_type, wrapper) = if file_path.ends_with(".mjs") {
        (
            "module",
            format!(
                r#"import {{ readFileSync }} from 'node:fs';
import {{ pathToFileURL }} from 'node:url';
const hooks = await import(pathToFileURL({file_path_escaped}).href);
const ctx = JSON.parse(readFileSync(0, 'utf8'));
const logger = {{
  info: (m) => {{ console.log(JSON.stringify({{"level":"info","message":String(m)}})); }},
  warn: (m) => {{ console.log(JSON.stringify({{"level":"warn","message":String(m)}})); }}
}};
await (hooks.hooks && hooks.hooks['{func}'])?.(ctx, logger);
"#,
            ),
        )
    } else {
        (
            "commonjs",
            format!(
                r#"(async () => {{
  const hooks = require({file_path_escaped});
  const ctx = JSON.parse(require('fs').readFileSync(0, 'utf8'));
  const logger = {{
info: (m) => {{ console.log(JSON.stringify({{"level":"info","message":String(m)}})); }},
warn: (m) => {{ console.log(JSON.stringify({{"level":"warn","message":String(m)}})); }}
  }};
  await (hooks.hooks && hooks.hooks['{func}'])?.(ctx, logger);
}})();
"#,
            ),
        )
    };
    Some((input_type, wrapper))
}

/// Feed the hook its context and collect its stderr tail and exit status.
async fn drive_hook(
    child: &mut tokio::process::Child,
    ctx_payload: &str,
    logger: &crate::PreResolutionHookLogger,
) -> Result<(Vec<u8>, std::io::Result<std::process::ExitStatus>), tokio::time::error::Elapsed> {
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    // Stream all three pipes concurrently instead of buffering:
    // stdout/stderr are hook-controlled, so buffering would let a noisy
    // pnpmfile grow memory without bound, and log messages should surface
    // while the hook is still running (pnpm runs the hook in-process, so
    // its logger calls render immediately). The context write runs in the
    // same join because a pnpmfile that logs heavily at import time could
    // otherwise fill the stdout pipe and deadlock against the stdin
    // write. A write error is left for `child.wait()` to surface as a
    // non-zero exit.
    let write_context = async {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(ctx_payload.as_bytes()).await;
        }
        // Dropping stdin closes the pipe so `readFileSync(0)` sees EOF.
    };
    let forward_stdout = forward_hook_stdout(stdout, logger);
    let collect_stderr = read_tail(stderr, STDERR_TAIL_LIMIT);
    let wait_child = child.wait();
    timeout(HOOK_TIMEOUT, async {
        let ((), (), stderr_tail, status) =
            tokio::join!(write_context, forward_stdout, collect_stderr, wait_child);
        (stderr_tail, status)
    })
    .await
}

#[async_trait]
impl crate::PnpmfileHooks for NodeJsHooks {
    async fn read_package(
        &self,
        pkg: Value,
        ctx: crate::HookContext,
    ) -> Result<crate::ReadPackageResult, HookError> {
        self.worker().await?.call("readPackage", pkg, ctx.log).await.map(Arc::new)
    }

    async fn after_all_resolved(
        &self,
        lockfile: Value,
        ctx: crate::HookContext,
    ) -> Result<Value, HookError> {
        self.worker().await?.call("afterAllResolved", lockfile, ctx.log).await
    }

    async fn update_config(
        &self,
        config: Value,
        ctx: crate::HookContext,
    ) -> Result<Value, HookError> {
        // The worker returns `null` when the pnpmfile exports no
        // `updateConfig` hook (the generic `typeof fn === 'function'`
        // branch); in that case the config is left unchanged.
        let result = self.worker().await?.call("updateConfig", config.clone(), ctx.log).await?;
        Ok(if result.is_null() { config } else { result })
    }

    async fn before_packing(
        &self,
        manifest: Value,
        dir: &std::path::Path,
        ctx: crate::HookContext,
    ) -> Result<Value, HookError> {
        self.worker().await?.call_before_packing(manifest, &dir.to_string_lossy(), ctx.log).await
    }

    async fn pre_resolution(
        &self,
        ctx: crate::PreResolutionHookContext,
        logger: crate::PreResolutionHookLogger,
    ) {
        let ctx_json = serde_json::json!({
            "wantedLockfile": ctx.wanted_lockfile,
            "currentLockfile": ctx.current_lockfile,
            "existsCurrentLockfile": ctx.exists_current_lockfile,
            "existsNonEmptyWantedLockfile": ctx.exists_non_empty_wanted_lockfile,
            "lockfileDir": ctx.lockfile_dir,
            "storeDir": ctx.store_dir,
            "registries": ctx.registries,
        });

        self.call_node_void("preResolution", ctx_json, &logger).await;
    }

    async fn filter_log(&self, log: Value, ctx: crate::HookContext) -> bool {
        let Ok(worker) = self.worker().await else { return true };
        match worker.call("filterLog", log, ctx.log).await {
            Ok(value) => value.as_bool().unwrap_or(true),
            Err(_) => true,
        }
    }

    async fn has_filter_log(&self) -> bool {
        match self.worker().await {
            Ok(worker) => worker.has_filter_log().await,
            Err(_) => false,
        }
    }

    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        // Gate on the loaded module exporting `hooks`, mirroring pnpm's
        // `entries.some(entry => entry.hooks != null)`. The checksum
        // value itself is a pure hash of the pnpmfile's normalized
        // bytes — only this gate needs to consult the evaluated module.
        let worker = self.worker().await.ok()?;
        if !worker.has_hooks().await {
            return None;
        }
        pnpm_crypto_hash::create_hash_from_file(&self.file).ok()
    }

    fn source_path(&self) -> Option<&std::path::Path> {
        Some(&self.file)
    }

    async fn get_custom_resolvers(&self) -> Result<Vec<Arc<dyn crate::CustomResolver>>, HookError> {
        let worker = self.worker().await?;
        let capabilities = worker.get_resolver_capabilities().await?;
        Ok(capabilities
            .into_iter()
            .enumerate()
            .map(|(index, capabilities)| {
                Arc::new(NodeJsCustomResolver { worker: Arc::clone(&worker), index, capabilities })
                    as Arc<dyn crate::CustomResolver>
            })
            .collect())
    }

    async fn get_custom_fetchers(&self) -> Result<Vec<Arc<dyn crate::CustomFetcher>>, HookError> {
        let worker = self.worker().await?;
        let capabilities = worker.get_fetcher_capabilities().await?;
        Ok(capabilities
            .into_iter()
            .enumerate()
            .map(|(index, capabilities)| {
                Arc::new(NodeJsCustomFetcher { worker: Arc::clone(&worker), index, capabilities })
                    as Arc<dyn crate::CustomFetcher>
            })
            .collect())
    }

    async fn get_finder_names(&self) -> Result<Vec<String>, HookError> {
        self.worker().await?.get_finder_names().await
    }

    async fn run_finder(&self, finder_name: &str, ctx: Value) -> Result<Value, HookError> {
        self.worker().await?.call_finder(finder_name, ctx).await
    }
}

pub struct NodeJsCustomResolver {
    worker: Arc<NodeWorker>,
    index: usize,
    capabilities: crate::worker::ResolverCapabilities,
}

#[async_trait]
impl crate::CustomResolver for NodeJsCustomResolver {
    fn has_can_resolve(&self) -> bool {
        self.capabilities.can_resolve
    }

    fn has_resolve(&self) -> bool {
        self.capabilities.resolve
    }

    fn has_should_refresh_resolution(&self) -> bool {
        self.capabilities.should_refresh_resolution
    }

    async fn can_resolve(&self, wanted_dependency: Value) -> Result<bool, HookError> {
        let res = self
            .worker
            .call_resolver(
                self.index,
                "canResolve",
                serde_json::json!([wanted_dependency]),
                Arc::new(|_| {}),
            )
            .await?;
        Ok(res.as_bool().unwrap_or(false))
    }

    async fn resolve(&self, wanted_dependency: Value, opts: Value) -> Result<Value, HookError> {
        self.worker
            .call_resolver(
                self.index,
                "resolve",
                serde_json::json!([wanted_dependency, opts]),
                Arc::new(|_| {}),
            )
            .await
    }

    async fn should_refresh_resolution(
        &self,
        dep_path: &pnpm_lockfile::PackageKey,
        pkg_snapshot: Value,
    ) -> Result<bool, HookError> {
        let res = self
            .worker
            .call_resolver(
                self.index,
                "shouldRefreshResolution",
                serde_json::json!([dep_path.to_string(), pkg_snapshot]),
                Arc::new(|_| {}),
            )
            .await?;
        Ok(res.as_bool().unwrap_or(false))
    }
}

pub struct NodeJsCustomFetcher {
    worker: Arc<NodeWorker>,
    index: usize,
    capabilities: crate::worker::FetcherCapabilities,
}

#[async_trait]
impl crate::CustomFetcher for NodeJsCustomFetcher {
    fn has_can_fetch(&self) -> bool {
        self.capabilities.can_fetch
    }

    fn has_fetch(&self) -> bool {
        self.capabilities.fetch
    }

    async fn can_fetch(&self, pkg_id: &str, resolution: Value) -> Result<bool, HookError> {
        let (can_fetch, _) = self.can_fetch_with_resolution(pkg_id, resolution).await?;
        Ok(can_fetch)
    }

    async fn can_fetch_with_resolution(
        &self,
        pkg_id: &str,
        resolution: Value,
    ) -> Result<(bool, Value), HookError> {
        let response = self
            .worker
            .call_fetcher(
                self.index,
                "canFetch",
                serde_json::json!([pkg_id, &resolution]),
                Arc::new(|_| {}),
                None,
            )
            .await?;
        let can_fetch = response.get("value").is_some_and(is_js_truthy);
        // A worker that answers without a `resolution` — the reply shape for a
        // fetcher whose `canFetch` went missing between capability probe and
        // call — leaves the caller's resolution untouched rather than blanking
        // it for every fetcher behind this one.
        let resolution = response.get("resolution").cloned().unwrap_or(resolution);
        Ok((can_fetch, resolution))
    }

    async fn fetch(
        &self,
        _pkg_id: &str,
        resolution: Value,
        opts: Value,
    ) -> Result<Value, HookError> {
        self.call_fetch(resolution, opts, None).await
    }

    async fn fetch_with_callbacks(
        &self,
        _pkg_id: &str,
        resolution: Value,
        opts: Value,
        callbacks: crate::FetcherCallbackSender,
    ) -> Result<Value, HookError> {
        self.call_fetch(resolution, opts, Some(callbacks)).await
    }
}

impl NodeJsCustomFetcher {
    /// The payload is positional to match the TypeScript hook signature
    /// `fetch(cafs, resolution, opts, fetchers)`. Slots 0 and 3 are placeholders
    /// the worker fills in: with `callbacks`, it substitutes a CAFS handle and
    /// the native tarball fetchers before calling the hook; without them, the
    /// hook sees `null` in both and answers with a `delegate` envelope instead.
    async fn call_fetch(
        &self,
        resolution: Value,
        opts: Value,
        callbacks: Option<crate::FetcherCallbackSender>,
    ) -> Result<Value, HookError> {
        self.worker
            .call_fetcher(
                self.index,
                "fetch",
                serde_json::json!([Value::Null, resolution, opts, Value::Null]),
                Arc::new(|_| {}),
                callbacks,
            )
            .await
    }
}

/// Match JavaScript's truthiness rules for JSON-representable values.
/// Falsy: `null`, `false`, `0`, `""`. Everything else is truthy.
fn is_js_truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests;

mod output;
use output::{STDERR_TAIL_LIMIT, forward_hook_stdout, read_tail};
