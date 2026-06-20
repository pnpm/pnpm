use async_trait::async_trait;
use derive_more::Display;
use serde_json::Value;
use std::sync::Arc;

pub mod custom_resolver_adapter;
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

    /// `updateConfig` hook: transforms the resolved configuration before
    /// install. Config-dependency plugins use it to inject settings such
    /// as `patchedDependencies` or `catalogs`.
    ///
    /// Returns the (possibly modified) config object. A hook-less
    /// pnpmfile returns `config` unchanged. A throwing hook yields a
    /// [`HookError`] and aborts the install. Mirrors pnpm's
    /// [`updateConfig` hook](https://github.com/pnpm/pnpm/blob/31858c544b/pnpm/src/getConfig.ts#L86-L91).
    async fn update_config(&self, config: Value, ctx: HookContext) -> Result<Value, HookError> {
        let _ = ctx;
        // The default no-op returns the config unchanged. Returning it
        // (rather than `Null`) keeps the chaining caller simple: every
        // hook takes and returns a config object.
        Ok(config)
    }

    /// `preResolution` hook: side-effect hook called before resolution (e.g., logging, validation).
    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger);

    /// `filterLog` hook: determines if a log message should be emitted.
    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool;

    /// Compute the `pnpmfileChecksum` recorded in `pnpm-lock.yaml`, or
    /// `None` when this hook set defines no `hooks` object.
    ///
    /// Mirrors pnpm's
    /// [`calculatePnpmfileChecksum`](https://github.com/pnpm/pnpm/blob/1819226b51/hooks/pnpmfile/src/requireHooks.ts#L131-L143):
    /// the checksum is installed (and thus written to the lockfile) only
    /// when at least one loaded pnpmfile exports a `hooks` object
    /// (`entries.some(entry => entry.hooks != null)`), and its value is
    /// the normalized-content hash of the included pnpmfiles. A pnpmfile
    /// that exists but exports no hooks contributes no checksum, matching
    /// pnpm.
    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        None
    }

    /// Path of the pnpmfile that defines these hooks, used as the `from`
    /// field of `pnpm:hook` log events. `None` for hook sets not backed by
    /// a file (e.g. the no-op).
    fn source_path(&self) -> Option<&std::path::Path> {
        None
    }

    /// Get custom resolvers exported from the pnpmfile's top-level
    /// `resolvers` array. Mirrors pnpm's
    /// [`requireHooks`](https://github.com/pnpm/pnpm/blob/1627943d2a/hooks/pnpmfile/src/requireHooks.ts#L222-L228)
    /// merge of `resolvers` exports into `cookedHooks.customResolvers`.
    async fn get_custom_resolvers(&self) -> Result<Vec<Arc<dyn CustomResolver>>, HookError> {
        Ok(vec![])
    }
}

/// A custom resolver exported from a pnpmfile. Mirrors pnpm's
/// [`CustomResolver`](https://github.com/pnpm/pnpm/blob/1627943d2a/hooks/types/src/index.ts#L48-L87)
/// interface, whose methods are all optional — the `has_*` accessors
/// report which ones the underlying resolver actually implements, so
/// callers can skip the corresponding calls the way pnpm skips absent
/// methods.
#[async_trait]
pub trait CustomResolver: Send + Sync {
    /// Whether the resolver implements `canResolve`.
    fn has_can_resolve(&self) -> bool {
        true
    }

    /// Whether the resolver implements `resolve`.
    fn has_resolve(&self) -> bool {
        true
    }

    /// Whether the resolver implements `shouldRefreshResolution`.
    fn has_should_refresh_resolution(&self) -> bool {
        true
    }

    /// Called during resolution to determine if this resolver should handle a dependency.
    async fn can_resolve(&self, wanted_dependency: Value) -> Result<bool, HookError>;

    /// Called to resolve a dependency that `canResolve` returned true for.
    async fn resolve(&self, wanted_dependency: Value, opts: Value) -> Result<Value, HookError>;

    /// Called on subsequent installs to determine if this dependency needs
    /// re-resolution. Invoked for every package in the lockfile regardless
    /// of `canResolve`; a `true` for any package forces full re-resolution.
    async fn should_refresh_resolution(
        &self,
        dep_path: &pacquet_lockfile::PackageKey,
        pkg_snapshot: Value,
    ) -> Result<bool, HookError>;
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
    async fn pre_resolution(&self, _: PreResolutionHookContext, _: PreResolutionHookLogger) {}
    async fn filter_log(&self, _: Value, _: HookContext) -> bool {
        true
    }
}
