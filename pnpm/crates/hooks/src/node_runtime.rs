use crate::{HookError, worker::NodeWorker};
use async_trait::async_trait;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
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
        let file_path = self.file.to_string_lossy();
        let Ok(file_path_escaped) = serde_json::to_string(&file_path) else { return };
        let Ok(ctx_payload) = serde_json::to_string(&args) else { return };

        let (input_type, wrapper) = if file_path.ends_with(".mjs") {
            (
                "module",
                format!(
                    r#"import {{ readFileSync }} from 'node:fs';
const hooks = await import({file_path_escaped});
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
        let hook_result = timeout(HOOK_TIMEOUT, async {
            let ((), (), stderr_tail, status) =
                tokio::join!(write_context, forward_stdout, collect_stderr, wait_child);
            (stderr_tail, status)
        })
        .await;

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

/// How much trailing stderr to keep for the failure message when the hook
/// exits non-zero.
const STDERR_TAIL_LIMIT: usize = 64 * 1024;

/// Longest hook stdout line kept; the remainder of an over-long line is
/// discarded so hook-controlled output cannot grow memory without bound.
const STDOUT_LINE_LIMIT: usize = 64 * 1024;

/// Forwards each of the one-shot hook's stdout lines to the Rust-side logger
/// closures as it arrives, which emit them as `pnpm:hook` events. Lines the
/// JS wrapper's logger writes carry their level; everything else the hook
/// prints (e.g. its own `console.log`) is forwarded as info so it is not
/// silently lost.
async fn forward_hook_stdout(
    stdout: tokio::process::ChildStdout,
    logger: &crate::PreResolutionHookLogger,
) {
    let mut reader = BufReader::new(stdout);
    while let Ok(Some(line)) = next_line_bounded(&mut reader, STDOUT_LINE_LIMIT).await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_logger_line(line) {
            Some((LoggerLevel::Info, message)) => (logger.info)(message),
            Some((LoggerLevel::Warn, message)) => (logger.warn)(message),
            None => (logger.info)(line.to_string()),
        }
    }
}

enum LoggerLevel {
    Info,
    Warn,
}

/// Parses one line of the JS wrapper's logger protocol,
/// `{"level":"info"|"warn","message":...}`. Anything else — non-JSON, or
/// JSON the hook printed itself — returns `None` so the caller forwards it
/// verbatim.
fn parse_logger_line(line: &str) -> Option<(LoggerLevel, String)> {
    if !line.starts_with('{') {
        return None;
    }
    let parsed = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let level = match parsed.get("level").and_then(|v| v.as_str()) {
        Some("info") => LoggerLevel::Info,
        Some("warn") => LoggerLevel::Warn,
        _ => return None,
    };
    let message = match parsed.get("message")? {
        v if v.is_string() => v.as_str().unwrap().to_string(),
        v => v.to_string(),
    };
    Some((level, message))
}

/// Reads one `\n`-terminated line, keeping at most `cap` bytes of it and
/// discarding the rest, and decodes it lossily. Unlike
/// [`AsyncBufReadExt::read_line`] this neither buffers an unbounded line nor
/// stops on invalid UTF-8 — the pipe must keep draining either way, or the
/// child blocks on a full pipe until the hook timeout. Returns `None` at EOF.
async fn next_line_bounded(
    reader: &mut (impl AsyncBufRead + Unpin),
    cap: usize,
) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let (consumed, line_complete) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                return Ok(if line.is_empty() { None } else { Some(lossy_string(&line)) });
            }
            let newline = available.iter().position(|&byte| byte == b'\n');
            let visible = newline.unwrap_or(available.len());
            let keep = visible.min(cap.saturating_sub(line.len()));
            line.extend_from_slice(&available[..keep]);
            match newline {
                Some(pos) => (pos + 1, true),
                None => (available.len(), false),
            }
        };
        reader.consume(consumed);
        if line_complete {
            return Ok(Some(lossy_string(&line)));
        }
    }
}

fn lossy_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Reads `stream` to EOF keeping only the final `cap` bytes, so a
/// hook-controlled pipe cannot grow the buffer without bound.
async fn read_tail(stream: impl AsyncRead + Unpin, cap: usize) -> Vec<u8> {
    let mut stream = stream;
    let mut tail = Vec::new();
    // Heap-allocated so the read buffer doesn't bloat the future
    // (`clippy::large_futures`).
    let mut buf = vec![0u8; 8192];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                tail.extend_from_slice(&buf[..n]);
                // Trim only past twice the cap so noisy output memmoves the
                // tail once per `cap` bytes, not once per read.
                if tail.len() > cap * 2 {
                    tail.drain(..tail.len() - cap);
                }
            }
        }
    }
    tail.drain(..tail.len().saturating_sub(cap));
    tail
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

    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        // Gate on the loaded module exporting `hooks`, mirroring pnpm's
        // `entries.some(entry => entry.hooks != null)`. The checksum
        // value itself is a pure hash of the pnpmfile's normalized
        // bytes — only this gate needs to consult the evaluated module.
        let worker = self.worker().await.ok()?;
        if !worker.has_hooks().await {
            return None;
        }
        pacquet_crypto_hash::create_hash_from_file(&self.file).ok()
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
        dep_path: &pacquet_lockfile::PackageKey,
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
        let res = self
            .worker
            .call_fetcher(
                self.index,
                "canFetch",
                serde_json::json!([pkg_id, resolution]),
                Arc::new(|_| {}),
            )
            .await?;
        Ok(is_js_truthy(&res))
    }

    async fn fetch(
        &self,
        _pkg_id: &str,
        resolution: Value,
        opts: Value,
    ) -> Result<Value, HookError> {
        // Positional parity with the TypeScript hook signature
        // `fetch(cafs, resolution, opts, fetchers)`: `cafs` and
        // `fetchers` cannot cross the IPC boundary, so they are `null`
        // placeholders — a portable pnpmfile fetcher detects their
        // absence and answers with `{ delegate: <resolution> }`.
        self.worker
            .call_fetcher(
                self.index,
                "fetch",
                serde_json::json!([Value::Null, resolution, opts, Value::Null]),
                Arc::new(|_| {}),
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
