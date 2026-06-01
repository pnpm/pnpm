use async_trait::async_trait;
use derive_more::Display;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    time::{Duration, timeout},
};

pub struct NodeJsHooks {
    pub file: PathBuf,
}

const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Display)]
pub enum HookError {
    #[display("pnpmfile hook '{_0}' timed out after {} seconds", HOOK_TIMEOUT.as_secs())]
    Timeout(String),

    ExecutionFailed(String),

    #[display("pnpmfile hook '{_0}' execution failed: {_1}")]
    HookFailed(String, String),
}

impl NodeJsHooks {
    async fn call_node(&self, func: &str, args: Value) -> Result<Value, HookError> {
        let payload = serde_json::to_string(&args)
            .map_err(|err| HookError::HookFailed(func.to_string(), err.to_string()))?;
        let file_path = self.file.to_string_lossy();
        let file_path_escaped = serde_json::to_string(&file_path)
            .map_err(|err| HookError::HookFailed(func.to_string(), err.to_string()))?;

        let (input_type, wrapper) = if file_path.ends_with(".mjs") {
            (
                "module",
                format!(
                    r#"const hooks = await import({file_path_escaped});
const res = await (hooks.hooks && hooks.hooks['{func}'])?.({payload});
console.log(JSON.stringify(res));
"#,
                    file_path_escaped = file_path_escaped,
                ),
            )
        } else {
            (
                "commonjs",
                format!(
                    r#"(async () => {{
  const hooks = require({file_path_escaped});
  const res = await (hooks.hooks && hooks.hooks['{func}'])?.({payload});
  console.log(JSON.stringify(res));
}})();
"#,
                    file_path_escaped = file_path_escaped,
                ),
            )
        };

        let output = timeout(
            HOOK_TIMEOUT,
            Command::new("node")
                .arg("--input-type")
                .arg(input_type)
                .arg("-e")
                .arg(&wrapper)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| HookError::Timeout(func.to_string()))?
        .map_err(|err| HookError::ExecutionFailed(err.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HookError::HookFailed(func.to_string(), stderr.to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();

        if stdout == "null" || stdout == "undefined" {
            return Ok(Value::Null);
        }

        serde_json::from_str(stdout)
            .map_err(|err| HookError::HookFailed(func.to_string(), err.to_string()))
    }

    async fn call_node_void(
        &self,
        func: &str,
        args: Value,
        logger: &crate::PreResolutionHookLogger,
    ) {
        let file_path = self.file.to_string_lossy();
        let file_path_escaped = match serde_json::to_string(&file_path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let ctx_payload = match serde_json::to_string(&args) {
            Ok(s) => s,
            Err(_) => return,
        };

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
                    file_path_escaped = file_path_escaped,
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
                    file_path_escaped = file_path_escaped,
                ),
            )
        };

        let mut child = match Command::new("node")
            .arg("--input-type")
            .arg(input_type)
            .arg("-e")
            .arg(&wrapper)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                (logger.warn)("pnpmfile hook failed to start".to_string());
                return;
            }
        };

        if let Some(mut stdin) = child.stdin.take()
            && stdin.write_all(ctx_payload.as_bytes()).await.is_err()
        {
            let _ = child.kill().await;
            return;
        }

        let output = match timeout(HOOK_TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(_)) | Err(_) => {
                (logger.warn)("pnpmfile hook timed out or failed to execute".to_string());
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            (logger.warn)(format!("pnpmfile hook failed: {}", stderr));
        }
    }
}

#[async_trait]
impl crate::PnpmfileHooks for NodeJsHooks {
    async fn read_package(
        &self,
        pkg: Value,
        ctx: crate::HookContext,
    ) -> Option<crate::ReadPackageResult> {
        match self.call_node("readPackage", pkg).await {
            Ok(v) if v.is_null() => None,
            Ok(v) => Some(Arc::new(v)),
            Err(err) => {
                (ctx.log)(format!("pnpmfile hook readPackage failed: {}", err));
                None
            }
        }
    }

    async fn after_all_resolved(&self, lockfile: Value, ctx: crate::HookContext) -> Option<Value> {
        match self.call_node("afterAllResolved", lockfile).await {
            Ok(v) => Some(v),
            Err(err) => {
                (ctx.log)(format!("pnpmfile hook afterAllResolved failed: {}", err));
                None
            }
        }
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
        match self.call_node("filterLog", log).await {
            Ok(v) => v.as_bool().unwrap_or(true),
            Err(err) => {
                (ctx.log)(format!("pnpmfile hook filterLog failed: {}", err));
                true
            }
        }
    }
}
