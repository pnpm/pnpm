use crate::HookError;
use async_trait::async_trait;
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

impl NodeJsHooks {
    fn pnpmfile_path(&self) -> String {
        self.file.to_string_lossy().into_owned()
    }

    fn execution_error(&self, message: impl Into<String>) -> HookError {
        HookError::Execution { pnpmfile: self.pnpmfile_path(), message: message.into() }
    }

    /// Runs `node` with the given wrapper script and returns trimmed stdout.
    ///
    /// A non-zero exit (a thrown hook, a syntax error, a missing `require`) is
    /// turned into [`HookError::Execution`] carrying the child's stderr, so the
    /// caller can abort the install with a meaningful message like pnpm does.
    async fn run_node(
        &self,
        func: &str,
        input_type: &str,
        wrapper: &str,
    ) -> Result<String, HookError> {
        let output = timeout(
            HOOK_TIMEOUT,
            Command::new("node")
                .arg("--input-type")
                .arg(input_type)
                .arg("-e")
                .arg(wrapper)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| HookError::Timeout(func.to_string(), HOOK_TIMEOUT.as_secs()))?
        .map_err(|err| self.execution_error(err.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(self.execution_error(stderr.trim()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    async fn call_node(&self, func: &str, args: Value) -> Result<Value, HookError> {
        let payload =
            serde_json::to_string(&args).map_err(|err| self.execution_error(err.to_string()))?;
        let file_path = self.file.to_string_lossy();
        let file_path_escaped = serde_json::to_string(&file_path)
            .map_err(|err| self.execution_error(err.to_string()))?;

        let (input_type, wrapper) = if file_path.ends_with(".mjs") {
            (
                "module",
                format!(
                    r#"const hooks = await import({file_path_escaped});
const res = await (hooks.hooks && hooks.hooks['{func}'])?.({payload});
console.log(JSON.stringify(res));
"#,
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
                ),
            )
        };

        let stdout = self.run_node(func, input_type, &wrapper).await?;
        if stdout == "null" || stdout == "undefined" {
            return Ok(Value::Null);
        }
        serde_json::from_str(&stdout).map_err(|err| self.execution_error(err.to_string()))
    }

    /// Runs the `readPackage` hook, mirroring pnpm's `requirePnpmfile` wrapper:
    /// the four dependency fields are defaulted to `{}` before the call, and the
    /// returned manifest is validated (must be a non-null object whose dependency
    /// fields, when present, are objects rather than arrays). A pnpmfile without a
    /// `readPackage` hook returns the manifest unchanged.
    async fn call_read_package(&self, pkg: Value) -> Result<Value, HookError> {
        let payload =
            serde_json::to_string(&pkg).map_err(|err| self.execution_error(err.to_string()))?;
        let file_path = self.file.to_string_lossy();
        let file_path_escaped = serde_json::to_string(&file_path)
            .map_err(|err| self.execution_error(err.to_string()))?;

        let body = format!(
            r#"const pkg = {payload};
const fn = mod.hooks && mod.hooks['readPackage'];
if (typeof fn !== 'function') {{ console.log(JSON.stringify(pkg)); }} else {{
  pkg.dependencies = pkg.dependencies ?? {{}};
  pkg.devDependencies = pkg.devDependencies ?? {{}};
  pkg.optionalDependencies = pkg.optionalDependencies ?? {{}};
  pkg.peerDependencies = pkg.peerDependencies ?? {{}};
  const newPkg = await fn(pkg, {{ log() {{}} }});
  if (!newPkg) {{
    throw new Error("readPackage hook did not return a package manifest object. Hook imported via " + {file_path_escaped});
  }}
  for (const dep of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]) {{
    const v = newPkg[dep];
    if (v != null && (typeof v !== "object" || Array.isArray(v))) {{
      throw new Error("readPackage hook returned package manifest object's property '" + dep + "' must be an object. Hook imported via " + {file_path_escaped});
    }}
  }}
  console.log(JSON.stringify(newPkg));
}}"#,
        );

        let (input_type, wrapper) = if file_path.ends_with(".mjs") {
            ("module", format!("const mod = await import({file_path_escaped});\n{body}"))
        } else {
            (
                "commonjs",
                format!(
                    "(async () => {{\nconst mod = require({file_path_escaped});\n{body}\n}})().catch((err) => {{ console.error(err && err.stack ? err.stack : String(err)); process.exit(1); }});",
                ),
            )
        };

        let stdout = self.run_node("readPackage", input_type, &wrapper).await?;
        serde_json::from_str(&stdout).map_err(|err| self.execution_error(err.to_string()))
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
        _ctx: crate::HookContext,
    ) -> Result<crate::ReadPackageResult, HookError> {
        self.call_read_package(pkg).await.map(Arc::new)
    }

    async fn after_all_resolved(
        &self,
        lockfile: Value,
        _ctx: crate::HookContext,
    ) -> Result<Value, HookError> {
        self.call_node("afterAllResolved", lockfile).await
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
