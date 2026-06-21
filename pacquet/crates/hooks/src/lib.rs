use std::{future::Future, sync::Arc};

use derive_more::Display;
use serde_json::Value;

pub mod custom_resolver_adapter;
pub mod finder;
pub mod node_runtime;
pub mod worker;

pub use node_runtime::NodeJsCustomResolver;
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
///
/// The concrete hook set is selected at runtime (a real pnpmfile or the
/// no-op) from a closed set, so callers hold the [`PnpmfileHooksKind`]
/// enum rather than a `dyn` trait object: every call dispatches
/// statically and the returned futures stay unboxed. Methods return
/// `impl Future + Send` (RPITIT with an explicit `Send` bound) so the
/// futures are usable across `tokio` tasks without the bound being
/// re-stated at each call site.
pub trait PnpmfileHooks: Send + Sync {
    /// `readPackage` hook: modifies a package manifest before it is used for resolution.
    ///
    /// Returns the (possibly modified) manifest. A hook that throws, or returns
    /// something other than a package manifest object, yields a [`HookError`] so
    /// the install fails loudly — matching pnpm, where a bad `readPackage` hook
    /// aborts resolution.
    fn read_package(
        &self,
        pkg: Value,
        ctx: HookContext,
    ) -> impl Future<Output = Result<ReadPackageResult, HookError>> + Send;

    /// `afterAllResolved` hook: modifies the final resolved lockfile.
    ///
    /// Returns the (possibly modified) lockfile. `Ok(Value::Null)` means the
    /// pnpmfile has no `afterAllResolved` hook, so the caller keeps the lockfile
    /// unchanged. A throwing hook yields a [`HookError`] and aborts the install.
    fn after_all_resolved(
        &self,
        lockfile: Value,
        ctx: HookContext,
    ) -> impl Future<Output = Result<Value, HookError>> + Send;

    /// `updateConfig` hook: transforms the resolved configuration before
    /// install. Config-dependency plugins use it to inject settings such
    /// as `patchedDependencies` or `catalogs`.
    ///
    /// Returns the (possibly modified) config object. A hook-less
    /// pnpmfile returns `config` unchanged. A throwing hook yields a
    /// [`HookError`] and aborts the install. Mirrors pnpm's
    /// [`updateConfig` hook](https://github.com/pnpm/pnpm/blob/31858c544b/pnpm/src/getConfig.ts#L86-L91).
    fn update_config(
        &self,
        config: Value,
        ctx: HookContext,
    ) -> impl Future<Output = Result<Value, HookError>> + Send {
        let _ = ctx;
        // The default no-op returns the config unchanged. Returning it
        // (rather than `Null`) keeps the chaining caller simple: every
        // hook takes and returns a config object.
        async move { Ok(config) }
    }

    /// `preResolution` hook: side-effect hook called before resolution (e.g., logging, validation).
    fn pre_resolution(
        &self,
        ctx: PreResolutionHookContext,
        logger: PreResolutionHookLogger,
    ) -> impl Future<Output = ()> + Send;

    /// `filterLog` hook: determines if a log message should be emitted.
    fn filter_log(&self, log: Value, ctx: HookContext) -> impl Future<Output = bool> + Send;

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
    fn calculate_pnpmfile_checksum(&self) -> impl Future<Output = Option<String>> + Send {
        async move { None }
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
    ///
    /// Every custom resolver is backed by the pnpmfile's Node worker, so
    /// the concrete [`NodeJsCustomResolver`] is returned directly — no
    /// trait object. Hook sets without a pnpmfile contribute none.
    fn get_custom_resolvers(
        &self,
    ) -> impl Future<Output = Result<Vec<NodeJsCustomResolver>, HookError>> + Send {
        async move { Ok(vec![]) }
    }
}

/// A custom resolver exported from a pnpmfile. Mirrors pnpm's
/// [`CustomResolver`](https://github.com/pnpm/pnpm/blob/1627943d2a/hooks/types/src/index.ts#L48-L87)
/// interface, whose methods are all optional — the `has_*` accessors
/// report which ones the underlying resolver actually implements, so
/// callers can skip the corresponding calls the way pnpm skips absent
/// methods.
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
    fn can_resolve(
        &self,
        wanted_dependency: Value,
    ) -> impl Future<Output = Result<bool, HookError>> + Send;

    /// Called to resolve a dependency that `canResolve` returned true for.
    fn resolve(
        &self,
        wanted_dependency: Value,
        opts: Value,
    ) -> impl Future<Output = Result<Value, HookError>> + Send;

    /// Called on subsequent installs to determine if this dependency needs
    /// re-resolution. Invoked for every package in the lockfile regardless
    /// of `canResolve`; a `true` for any package forces full re-resolution.
    fn should_refresh_resolution(
        &self,
        dep_path: &pacquet_lockfile::PackageKey,
        pkg_snapshot: Value,
    ) -> impl Future<Output = Result<bool, HookError>> + Send;
}

/// A no-op implementation of `PnpmfileHooks`.
pub struct NoopHooks;

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

/// The selected pnpmfile hook set. A closed set chosen at runtime —
/// either a real pnpmfile run through Node.js or the [`NoopHooks`]
/// fallback — held as a concrete `enum` so [`PnpmfileHooks`] calls
/// dispatch statically and the returned futures stay unboxed. See
/// [`finder::load_pnpmfile`] for the selection.
pub enum PnpmfileHooksKind {
    NodeJs(node_runtime::NodeJsHooks),
    Noop(NoopHooks),
}

impl PnpmfileHooks for PnpmfileHooksKind {
    async fn read_package(
        &self,
        pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.read_package(pkg, ctx).await,
            PnpmfileHooksKind::Noop(hooks) => hooks.read_package(pkg, ctx).await,
        }
    }

    async fn after_all_resolved(
        &self,
        lockfile: Value,
        ctx: HookContext,
    ) -> Result<Value, HookError> {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.after_all_resolved(lockfile, ctx).await,
            PnpmfileHooksKind::Noop(hooks) => hooks.after_all_resolved(lockfile, ctx).await,
        }
    }

    async fn update_config(&self, config: Value, ctx: HookContext) -> Result<Value, HookError> {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.update_config(config, ctx).await,
            PnpmfileHooksKind::Noop(hooks) => hooks.update_config(config, ctx).await,
        }
    }

    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.pre_resolution(ctx, logger).await,
            PnpmfileHooksKind::Noop(hooks) => hooks.pre_resolution(ctx, logger).await,
        }
    }

    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.filter_log(log, ctx).await,
            PnpmfileHooksKind::Noop(hooks) => hooks.filter_log(log, ctx).await,
        }
    }

    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.calculate_pnpmfile_checksum().await,
            PnpmfileHooksKind::Noop(hooks) => hooks.calculate_pnpmfile_checksum().await,
        }
    }

    fn source_path(&self) -> Option<&std::path::Path> {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.source_path(),
            PnpmfileHooksKind::Noop(hooks) => hooks.source_path(),
        }
    }

    async fn get_custom_resolvers(&self) -> Result<Vec<NodeJsCustomResolver>, HookError> {
        match self {
            PnpmfileHooksKind::NodeJs(hooks) => hooks.get_custom_resolvers().await,
            PnpmfileHooksKind::Noop(hooks) => hooks.get_custom_resolvers().await,
        }
    }
}
