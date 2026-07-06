//! JS-backed `readPackage` hook.
//!
//! Bit transforms every dependency manifest during resolution (it strips
//! `@teambit/legacy` / `@teambit/harmony` and reshapes workspace-package deps).
//! pacquet applies such transforms through the [`PnpmfileHooks`] trait, which
//! the CLI loads from a `.pnpmfile.cjs`. [`JsReadPackageHook`] instead adapts a
//! host-supplied JS callback: `read_package` forwards each manifest to the
//! callback over a [`ThreadsafeFunction`] and awaits the transformed result.
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
use napi::{Status, threadsafe_function::ThreadsafeFunction};
use pacquet_hooks::{
    HookContext, HookError, PnpmfileHooks, PreResolutionHookContext, PreResolutionHookLogger,
    ReadPackageResult,
};
use serde_json::Value;

/// A synchronous JS `(manifest) => manifest` callback. `CalleeHandled = false`
/// (no leading error arg); the JS return value is deserialized back to a
/// manifest.
pub type HookSink = ThreadsafeFunction<Value, Value, Value, Status, false>;

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
        _ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        match self.read_package.call_async(pkg).await {
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
