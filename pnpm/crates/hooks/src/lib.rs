use async_trait::async_trait;
use derive_more::Display;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

pub mod custom_fetcher_adapter;
pub mod custom_resolver_adapter;
pub mod finder;
pub mod node_runtime;
pub mod worker;

pub use worker::LogFn;

/// A native operation requested by a JavaScript custom fetcher.
pub struct FetcherCallback {
    pub method: FetcherMethod,
    pub resolution: Value,
    pub options: Value,
    pub response: oneshot::Sender<Result<Value, Value>>,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FetcherMethod {
    CafsInfo,
    TempDir,
    LocalTarball,
    RemoteTarball,
}

pub type FetcherCallbackSender = mpsc::UnboundedSender<FetcherCallback>;

/// Represents the results of a `readPackage` hook.
pub type ReadPackageResult = Arc<Value>;

/// An error raised while running a pnpmfile hook in Node.js.
///
/// Covers the `ERR_PNPM_PNPMFILE_FAIL` / `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT` conditions: a
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
#[derive(Clone)]
pub struct HookContext {
    pub log: Arc<dyn Fn(String) + Send + Sync>,
    /// Lockfile-root-relative directory of the resolution, set when the
    /// manifest being transformed was resolved from a local directory (an
    /// injected workspace project or a `file:` dependency). A host-supplied
    /// `readPackage` callback uses it to recognize a workspace project's
    /// dependency instance and substitute the project's raw manifest.
    /// Only the node-API bridge forwards it to JS; the `.pnpmfile.cjs`
    /// contract (`readPackage(pkg, context)`) has no directory, matching
    /// pnpm.
    pub dir: Option<String>,
}

/// Logger for preResolution hook (info/warn methods).
#[derive(Clone)]
pub struct PreResolutionHookLogger {
    pub info: Arc<dyn Fn(String) + Send + Sync>,
    pub warn: Arc<dyn Fn(String) + Send + Sync>,
}

/// Context provided to preResolution hooks.
#[derive(Clone)]
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

    /// `beforePacking` hook: transforms a project's published manifest
    /// before it is packed. Publish and pack run it once per project,
    /// after the exportable manifest is built and before the file list
    /// is computed, so a hook may still change `files`, `bin`, or the
    /// dependency fields.
    ///
    /// Called with `(manifest, dir, context)`, mirroring pnpm's cooked
    /// `beforePacking(pkg, dir, context)` signature. Returns the
    /// (possibly modified) manifest; a hook that returns nothing — and
    /// the default no-op — leaves `manifest` unchanged. A throwing hook
    /// yields a [`HookError`] and aborts packing.
    async fn before_packing(
        &self,
        manifest: Value,
        dir: &std::path::Path,
        ctx: HookContext,
    ) -> Result<Value, HookError> {
        let _ = (dir, ctx);
        Ok(manifest)
    }

    /// `preResolution` hook: side-effect hook called before resolution (e.g., logging, validation).
    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger);

    /// `filterLog` hook: determines if a log message should be emitted.
    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool;

    /// Whether this pnpmfile exports a callable `filterLog` hook.
    async fn has_filter_log(&self) -> bool {
        false
    }

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

    /// The names of the finders exported from the pnpmfile's top-level
    /// `finders` object (consumed by `pnpm list --find-by` /
    /// `pnpm why --find-by`).
    async fn get_finder_names(&self) -> Result<Vec<String>, HookError> {
        Ok(vec![])
    }

    /// Run the finder named `finder_name` against one package. `ctx`
    /// carries `alias`, `name`, `version`, and the package's `manifest`
    /// (exposed to the JavaScript finder as its `readManifest()`
    /// callback). Returns the finder's verdict: a boolean or a message
    /// string.
    async fn run_finder(&self, finder_name: &str, ctx: Value) -> Result<Value, HookError> {
        let _ = (finder_name, ctx);
        Ok(Value::Bool(false))
    }
}

/// A custom fetcher exported from a pnpmfile's `fetchers` array.
///
/// Custom fetchers are consulted before the built-in fetchers. If `can_fetch`
/// returns `true`, `fetch` is called with the possibly modified resolution.
///
/// The pnpmfile hook is invoked with the same positional arguments as the
/// TypeScript CLI's `CustomFetcher.fetch(cafs, resolution, opts, fetchers)`
/// (`pnpm11/hooks/types/src/index.ts`). During installation, built-in tarball
/// fetches cross the worker IPC boundary and finish before the callback returns.
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

    /// Preserve changes a JavaScript `canFetch` hook makes to its resolution.
    async fn can_fetch_with_resolution(
        &self,
        pkg_id: &str,
        resolution: Value,
    ) -> Result<(bool, Value), HookError> {
        let can_fetch = self.can_fetch(pkg_id, resolution.clone()).await?;
        Ok((can_fetch, resolution))
    }

    /// Calls the fetcher hook. The returned JSON envelope is interpreted by the
    /// installer:
    ///
    /// - `{ "delegate": <resolution> }` — rewrites the lockfile resolution and
    ///   falls through to the built-in fetch path for the rewritten value.
    /// - A built-in fetch result containing `filesMap` is reused directly.
    /// - Any other shape fails the install (`custom_fetcher_failed`).
    async fn fetch(&self, pkg_id: &str, resolution: Value, opts: Value)
    -> Result<Value, HookError>;

    /// Run a fetch with native callbacks supplied by the installer.
    async fn fetch_with_callbacks(
        &self,
        pkg_id: &str,
        resolution: Value,
        opts: Value,
        _callbacks: FetcherCallbackSender,
    ) -> Result<Value, HookError> {
        self.fetch(pkg_id, resolution, opts).await
    }
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
        dep_path: &pnpm_lockfile::PackageKey,
        pkg_snapshot: Value,
    ) -> Result<bool, HookError>;
}

/// The `pnpmfileChecksum` an install through `hooks` would record in
/// `pnpm-lock.yaml`, for comparison against the `recorded` value in the
/// lockfile a freshness gate is checking.
///
/// [`PnpmfileHooks::calculate_pnpmfile_checksum`] evaluates the
/// pnpmfile to answer whether it exports hooks, which costs a Node
/// worker. `recorded` settles the comparison without it whenever the
/// lockfile already holds a checksum: a pnpmfile whose bytes still hash
/// to that value is the same module that produced it and still exports
/// hooks, while one that hashes differently is drift whether or not it
/// exports any. Only a lockfile that records no checksum needs the
/// pnpmfile evaluated, to tell "no pnpmfile" from "a pnpmfile that
/// exports none".
pub async fn current_pnpmfile_checksum(
    hooks: Option<&Arc<dyn PnpmfileHooks>>,
    recorded: Option<&str>,
) -> Option<String> {
    let hooks = hooks?;
    if recorded.is_some()
        && let Some(file) = hooks.source_path()
        && let Ok(hash) = pnpm_crypto_hash::create_hash_from_file(file)
    {
        return Some(hash);
    }
    hooks.calculate_pnpmfile_checksum().await
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
