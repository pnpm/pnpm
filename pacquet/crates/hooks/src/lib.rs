use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub mod finder;
pub mod node_runtime;

/// Represents the results of a `readPackage` hook.
pub type ReadPackageResult = Arc<Value>;

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
    async fn read_package(&self, pkg: Value, ctx: HookContext) -> Option<ReadPackageResult>;

    /// `afterAllResolved` hook: modifies the final resolved lockfile.
    async fn after_all_resolved(&self, lockfile: Value, ctx: HookContext) -> Option<Value>;

    /// `preResolution` hook: side-effect hook called before resolution (e.g., logging, validation).
    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger);

    /// `filterLog` hook: determines if a log message should be emitted.
    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool;
}

/// A no-op implementation of `PnpmfileHooks`.
pub struct NoopHooks;

#[async_trait]
impl PnpmfileHooks for NoopHooks {
    async fn read_package(&self, _: Value, _: HookContext) -> Option<ReadPackageResult> {
        None
    }
    async fn after_all_resolved(&self, _: Value, _: HookContext) -> Option<Value> {
        None
    }
    async fn pre_resolution(&self, _: PreResolutionHookContext, _: PreResolutionHookLogger) {
        // no-op
    }
    async fn filter_log(&self, _: Value, _: HookContext) -> bool {
        true
    }
}
