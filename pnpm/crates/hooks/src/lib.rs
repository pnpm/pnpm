use async_trait::async_trait;
use derive_more::Display;
use serde_json::Value;
use std::sync::Arc;

pub mod custom_fetcher_adapter;
pub mod custom_resolver_adapter;
pub mod finder;
pub mod node_runtime;
pub mod worker;

pub use worker::LogFn;

/// Represents the results of a `readPackage` hook.
pub type ReadPackageResult = Arc<Value>;

/// An error raised while running a pnpmfile hook in Node.js.
///
/// Covers the `PNPMFILE_FAIL` / `BAD_READ_PACKAGE_HOOK_RESULT` conditions: a
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
    /// Lockfile-root-relative directory of the resolution, set when the
    /// manifest being transformed was resolved from a local directory (an
    /// injected workspace project or a `file:` dependency). A host-supplied
    /// `readPackage` callback uses it to recognize a workspace project's
    /// dependency instance and substitute the project's raw manifest —
    /// pnpm's TS engine reaches the same effect by keeping raw project
    /// manifests and applying its hook chain contextually. Only the
    /// node-API bridge forwards it to JS; the `.pnpmfile.cjs` contract
    /// (`readPackage(pkg, context)`) has no directory, matching pnpm.
    pub dir: Option<String>,
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
    /// the install fails loudly — a bad `readPackage` hook aborts resolution.
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
    /// [`HookError`] and aborts the install.
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
    /// The checksum is installed (and thus written to the lockfile) only
    /// when at least one loaded pnpmfile exports a `hooks` object, and its
    /// value is the normalized-content hash of the included pnpmfiles. A
    /// pnpmfile that exists but exports no hooks contributes no checksum.
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
    /// `resolvers` array.
    async fn get_custom_resolvers(&self) -> Result<Vec<Arc<dyn CustomResolver>>, HookError> {
        Ok(vec![])
    }

    /// Get custom fetchers exported from the pnpmfile's top-level
    /// `fetchers` array.
    async fn get_custom_fetchers(&self) -> Result<Vec<Arc<dyn CustomFetcher>>, HookError> {
        Ok(vec![])
    }
}

/// A custom fetcher exported from a pnpmfile's `fetchers` array.
///
/// Custom fetchers are consulted before the built-in fetchers. If `can_fetch`
/// returns `true`, `fetch` is called. Currently only delegation is supported:
/// the fetcher returns `{ "delegate": <LockfileResolution> }` to rewrite the
/// resolution and fall through to the built-in fetch path.
///
/// The pnpmfile hook is invoked with the same positional arguments as the
/// TypeScript CLI's `CustomFetcher.fetch(cafs, resolution, opts, fetchers)`
/// (`pnpm11/hooks/types/src/index.ts`); `cafs` and `fetchers` are `null`
/// placeholders because they cannot cross the worker IPC boundary, which is
/// how a portable fetcher knows to delegate instead of fetching directly.
#[async_trait]
pub trait CustomFetcher: Send + Sync {
    fn has_can_fetch(&self) -> bool {
        true
    }

    fn has_fetch(&self) -> bool {
        true
    }

    /// Determines whether this fetcher handles the given package.
    async fn can_fetch(&self, pkg_id: &str, resolution: Value) -> Result<bool, HookError>;

    /// Calls the fetcher hook. The returned JSON envelope is interpreted by the
    /// installer:
    ///
    /// - `{ "delegate": <resolution> }` — rewrites the lockfile resolution and
    ///   falls through to the built-in fetch path for the rewritten value.
    /// - Any other shape fails the install (`custom_fetcher_failed`): a fetcher
    ///   that claims a package via [`CustomFetcher::can_fetch`] must delegate,
    ///   because direct content fetch isn't supported yet.
    ///
    /// The built-in fetch path runs with the original resolution unchanged
    /// only when no fetcher claims the package.
    async fn fetch(&self, pkg_id: &str, resolution: Value, opts: Value)
    -> Result<Value, HookError>;
}

/// A custom resolver exported from a pnpmfile. The pnpmfile interface's
/// methods are all optional — the `has_*` accessors report which ones the
/// underlying resolver actually implements, so callers can skip the
/// corresponding calls for absent methods.
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

/// A no-op implementation of [`PnpmfileHooks`].
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
