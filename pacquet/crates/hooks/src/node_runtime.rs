use crate::{HookError, worker::NodeWorker};
use async_trait::async_trait;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};
use tokio::{
    io::AsyncWriteExt,
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
  info: (m) => {{ console.log(JSON.stringify({{"level":"info","message":m}})); }},
  warn: (m) => {{ console.log(JSON.stringify({{"level":"warn","message":m}})); }}
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
    info: (m) => {{ console.log(JSON.stringify({{"level":"info","message":m}})); }},
    warn: (m) => {{ console.log(JSON.stringify({{"level":"warn","message":m}})); }}
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
            .spawn()
        else {
            (logger.warn)("pnpmfile hook failed to start".to_string());
            return;
        };

        if let Some(mut stdin) = child.stdin.take()
            && stdin.write_all(ctx_payload.as_bytes()).await.is_err()
        {
            let _ = child.kill().await;
            return;
        }

        let Ok(Ok(output)) = timeout(HOOK_TIMEOUT, child.wait_with_output()).await else {
            (logger.warn)("pnpmfile hook timed out or failed to execute".to_string());
            return;
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            (logger.warn)(format!("pnpmfile hook failed: {stderr}"));
        }
    }
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
}
