use async_trait::async_trait;
use derive_more::Display;
use serde_json::Value;
use std::sync::Arc;

pub mod finder;
pub mod node_runtime;
pub mod worker;

pub use worker::LogFn;

/// Represents the results of a `readPackage` hook.
pub type ReadPackageResult = Arc<Value>;

/// An error raised while running a pnpmfile hook in Node.js.
///
/// Mirrors pnpm's `PNPMFILE_FAIL` / `BAD_READ_PACKAGE_HOOK_RESULT` errors: a
/// throwing or syntactically invalid pnpmfile, or a `readPackage` hook that
/// returns something that is not a package manifest, aborts the install.
#[derive(Debug, Display, Clone)]
pub enum HookError {
    #[display("pnpmfile hook '{_0}' timed out after {_1} seconds")]
    Timeout(String, u64),

    #[display("Error during pnpmfile execution. pnpmfile: \"{pnpmfile}\". Error: \"{message}\".")]
    Execution { pnpmfile: String, message: String },
}

/// Context provided to pnpmfile hooks.
pub struct HookContext {
    pub log: Arc<dyn Fn(String) + Send + Sync>,
}

/// Logger for preResolution hook (info/warn methods).
pub struct PreResolutionHookLogger {
    pub info: Arc<dyn Fn(String) + Send + Sync>,
    pub warn: Arc<dyn Fn(String) + Send + Sync>,
}

/// Context provided to preResolution hooks.
pub struct PreResolutionHookContext {
    pub wanted_lockfile: Value,
    pub current_lockfile: Value,
    pub exists_current_lockfile: bool,
    pub exists_non_empty_wanted_lockfile: bool,
    pub lockfile_dir: String,
    pub store_dir: String,
    pub registries: Value,
}

/// The surface of hooks provided by `.pnpmfile.cjs` / `pnpmfile.cjs`.
#[async_trait]
pub trait PnpmfileHooks: Send + Sync {
    /// `readPackage` hook: modifies a package manifest before it is used for resolution.
    ///
    /// Returns the (possibly modified) manifest. A hook that throws, or returns
    /// something other than a package manifest object, yields a [`HookError`] so
    /// the install fails loudly — matching pnpm, where a bad `readPackage` hook
    /// aborts resolution.
    async fn read_package(
        &self,
        pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError>;

    /// `afterAllResolved` hook: modifies the final resolved lockfile.
    ///
    /// Returns the (possibly modified) lockfile. `Ok(Value::Null)` means the
    /// pnpmfile has no `afterAllResolved` hook, so the caller keeps the lockfile
    /// unchanged. A throwing hook yields a [`HookError`] and aborts the install.
    async fn after_all_resolved(
        &self,
        lockfile: Value,
        ctx: HookContext,
    ) -> Result<Value, HookError>;

    /// `preResolution` hook: side-effect hook called before resolution (e.g., logging, validation).
    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger);

    /// `filterLog` hook: determines if a log message should be emitted.
    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool;

    /// Path of the pnpmfile that defines these hooks, used as the `from`
    /// field of `pnpm:hook` log events. `None` for hook sets not backed by
    /// a file (e.g. the no-op).
    fn source_path(&self) -> Option<&std::path::Path> {
        None
    }
}

/// A no-op implementation of `PnpmfileHooks`.
pub struct NoopHooks;

#[async_trait]
impl PnpmfileHooks for NoopHooks {
    async fn read_package(
        &self,
        pkg: Value,
        _: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        Ok(Arc::new(pkg))
    }
    async fn after_all_resolved(&self, _: Value, _: HookContext) -> Result<Value, HookError> {
        Ok(Value::Null)
    }
    async fn pre_resolution(&self, _: PreResolutionHookContext, _: PreResolutionHookLogger) {
        // no-op
    }
    async fn filter_log(&self, _: Value, _: HookContext) -> bool {
        true
    }
}
