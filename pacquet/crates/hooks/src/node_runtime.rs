use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

pub struct NodeJsHooks {
    pub file: PathBuf,
}

impl NodeJsHooks {
    fn call_node(&self, func: &str, args: Value) -> Result<Value, String> {
        let payload = serde_json::to_string(&args).map_err(|e| e.to_string())?;

        let wrapper = format!(
            r##"
const hooks = require('{}');
const res = (hooks.hooks && hooks.hooks['{}'])?.({});
console.log(JSON.stringify(res));
"##,
            self.file.to_string_lossy(),
            func,
            payload
        );

        let output =
            Command::new("node").arg("-e").arg(&wrapper).output().map_err(|e| e.to_string())?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(stderr.to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();

        // Handle undefined/null return (hook returns nothing)
        if stdout == "null" || stdout == "undefined" {
            return Ok(Value::Null);
        }

        serde_json::from_str(stdout).map_err(|e| e.to_string())
    }
}

#[async_trait]
impl crate::PnpmfileHooks for NodeJsHooks {
    async fn read_package(
        &self,
        pkg: Value,
        _ctx: crate::HookContext,
    ) -> Option<crate::ReadPackageResult> {
        self.call_node("readPackage", pkg).ok().and_then(|v| {
            if v.is_null() {
                None
            } else {
                Some(std::sync::Arc::new(v))
            }
        })
    }

    async fn after_all_resolved(&self, lockfile: Value, _ctx: crate::HookContext) -> Option<Value> {
        self.call_node("afterAllResolved", lockfile).ok()
    }

    async fn pre_resolution(&self, _ctx: crate::HookContext) -> Option<Value> {
        self.call_node("preResolution", Value::Null).ok()
    }

    async fn filter_log(&self, log: Value, _ctx: crate::HookContext) -> bool {
        self.call_node("filterLog", log).map(|v| v.as_bool().unwrap_or(true)).unwrap_or(true)
    }
}
