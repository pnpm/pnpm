pub use runtime::{host_platform_selector, runtime_platform_selector};
pub(crate) use tarball_resolution::local_file_tarball_install_url;
pub use tarball_resolution::{tarball_url_and_integrity, unverified_fetch_is_allowed};

mod fetch;

mod runtime;

mod tarball_resolution;

use crate::{CreateVirtualDirError, CustomFetcherSession};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_directory_fetcher::DirectoryFetcherError;
use pnpm_git_fetcher::GitFetcherError;
use pnpm_lockfile::PlatformSelector;
use pnpm_network::ThrottledClient;
use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pnpm_tarball::{MemCache, PrefetchedCasPaths, SharedReportedProgressKeys, TarballError};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

/// The running pnpm, which a git-hosted dependency's build is given so it
/// can install with the package manager it asks for.
///
/// `None` when the executable is something other than pnpm itself — the
/// Node.js addon runs this code inside `node`, where there is no pnpm
/// binary to forward to — and the build then falls back to whatever
/// package managers the host has installed.
static PNPM_EXECPATH: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    let path = std::env::current_exe().ok()?;
    let stem = path.file_stem()?.to_str()?;
    (stem == "pnpm").then_some(path)
});

/// Downloads a package tarball, extracts it, installs it to a virtual
/// dir, then creates the symlink layout for the package. CAS file
/// import and symlink creation run concurrently via `rayon::join`
/// inside [`CreateVirtualDirBySnapshot::run`](crate::CreateVirtualDirBySnapshot::run).
///
/// Holds only what every snapshot of one install shares — the clients,
/// the store handles, the layout, the policies — so it is built once
/// and each snapshot is passed to [`Self::run`].
#[derive(Clone, Copy)]
pub struct InstallPackageBySnapshot<'a> {
    pub ctx: &'a crate::InstallContext<'a>,
    pub http_client: &'a ThrottledClient,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Install-scoped batched cache lookup result. See
    /// [`pnpm_tarball::prefetch_cas_paths`].
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    /// Install-scoped shared in-flight tarball cache. When present, the
    /// registry/tarball download routes through
    /// [`IngestTarballToStore::run_with_mem_cache`](pnpm_tarball::IngestTarballToStore::run_with_mem_cache) so it parks on (or
    /// reuses) a download already in flight or completed for the same
    /// URL, rather than racing a second fetch of the same bytes. Both
    /// background prefetchers feed it: the pnpr client's
    /// `TarballPrefetcher` (frozen materialization) and the
    /// fresh-resolve path's `PrefetchingResolver` (cold
    /// batch). `None` keeps the standalone `run_without_mem_cache`
    /// path for installs with no prefetcher (e.g. a plain
    /// `--frozen-lockfile` without pnpr).
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
    /// Install-scoped package-status progress dedupe. Shared with the
    /// resolve-time prefetcher on the fresh path so the cold fallback
    /// does not double-count a package whose early prefetch already
    /// emitted `fetched` or `found_in_store`.
    pub progress_reported: Option<&'a SharedReportedProgressKeys>,
    /// Install-scoped `verifiedFilesCache` shared across every
    /// per-snapshot fetch. See `IngestTarballToStore::verified_files_cache`
    /// for the rationale.
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    /// Snapshots the installability pass ruled out on this host.
    pub skipped: &'a crate::SkippedSnapshots,
    pub include_optional_dependencies: bool,
    /// Platform triple used to select a runtime archive. This is the host
    /// triple unless `supportedArchitectures` targets another platform.
    pub runtime_platform_selector: &'a PlatformSelector,
    /// Custom fetchers from the pnpmfile's `fetchers` export.
    /// Consulted before the built-in resolution-type dispatch; `None`
    /// when no pnpmfile exports fetchers.
    pub custom_fetcher_session: Option<&'a Arc<CustomFetcherSession>>,
    /// When `true`, return the fetched CAS paths without populating the
    /// virtual-store slot ([`CreateVirtualDirBySnapshot`](crate::CreateVirtualDirBySnapshot)) — the caller
    /// links them itself in a separate parallel pass. The cold batch in
    /// [`crate::CreateVirtualStore`] sets this so the per-snapshot
    /// download futures don't each run a *blocking* `rayon::join` link
    /// inside the cooperative `try_join_all` task, which would serialize
    /// the links one-at-a-time; instead every slot links concurrently
    /// once its tarball is in the store. No effect under
    /// [`NodeLinker::Hoisted`](pnpm_modules_yaml::NodeLinker::Hoisted), which never writes virtual-store slots.
    pub defer_link: bool,
    #[cfg(test)]
    pub link_concurrency_probe:
        Option<&'a crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe>,
}

/// Error type of [`InstallPackageBySnapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallPackageBySnapshotError {
    #[diagnostic(transparent)]
    DownloadTarball(#[error(source)] TarballError),

    #[diagnostic(transparent)]
    CreateVirtualDir(#[error(source)] CreateVirtualDirError),

    /// A plain remote tarball the lockfile pins no `integrity` for.
    /// Message and code mirror the TypeScript
    /// `assertFetchableResolution` in
    /// `pnpm11/installing/package-requester/src/packageRequester.ts`.
    /// See [`unverified_fetch_is_allowed`] for the shapes that are
    /// exempt.
    #[display(
        "Cannot fetch package \"{package_key}\" from the lockfile: it has no \"integrity\" field, so the downloaded tarball cannot be verified. Run a fresh install to repair the lockfile."
    )]
    #[diagnostic(
        code(ERR_PNPM_MISSING_TARBALL_INTEGRITY),
        help(
            "Re-resolving the entry is what records the missing hash: run `pnpm clean --lockfile` and then `pnpm install`."
        )
    )]
    MissingTarballIntegrity { package_key: String },

    #[display(
        "Cannot install package \"{package_key}\": its registry prefix '{registry_name}:' is not declared by the registries setting."
    )]
    #[diagnostic(
        code(ERR_PNPM_MISSING_NAMED_REGISTRY),
        help("Add a registries entry with \"prefix: {registry_name}\" to pnpm-workspace.yaml.")
    )]
    MissingNamedRegistry { package_key: String, registry_name: String },

    #[display(
        "Cannot install package \"{package_key}\": its lockfile entry with a revision {reason}."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_TARBALL_REVISION))]
    InvalidTarballRevision { package_key: String, reason: &'static str },

    #[display(
        "Package `{package_key}` uses a `{resolution_kind}` resolution, which pnpm does not yet support."
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_UNSUPPORTED_RESOLUTION))]
    UnsupportedResolution { package_key: String, resolution_kind: &'static str },

    /// Failure from either git fetcher: the git-CLI path for
    /// `type: git` resolutions (clone / checkout / preparePackage /
    /// CAS import) or the git-hosted-tarball post-pass for
    /// `TarballResolution { gitHosted: true }` (materialize /
    /// preparePackage / packlist / re-import). Both share the same
    /// `GitFetcherError` taxonomy because they share `prepare_package`,
    /// `packlist`, and the CAS-import helpers; the variant covers
    /// every fetcher path that exits through `pnpm-git-fetcher`.
    #[diagnostic(transparent)]
    GitFetch(#[error(source)] GitFetcherError),

    /// Failure from the directory fetcher: walking the source
    /// directory of an injected workspace dep, reading its manifest,
    /// or running the npm-packlist filter for
    /// `includeOnlyPackageFiles` mode.
    #[diagnostic(transparent)]
    DirectoryFetch(#[error(source)] DirectoryFetcherError),

    /// A custom fetcher from the pnpmfile threw or returned an error.
    #[display("Custom fetcher failed: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CUSTOM_FETCHER_FAILED))]
    CustomFetcher(#[error(not(source))] String),

    #[display(
        "Custom fetcher delegated package \"{package_id}\" to a resolution that cannot verify its locked integrity"
    )]
    #[diagnostic(code(ERR_PNPM_TARBALL_INTEGRITY))]
    CustomFetcherIntegrityMismatch { package_id: String },

    /// A custom-typed resolution reached the built-in dispatch — no
    /// pnpmfile custom fetcher claimed it. Message and code mirror the
    /// TypeScript `pickFetcher` in
    /// `pnpm11/fetching/pick-fetcher/src/index.ts`.
    #[display(
        "Cannot fetch dependency with custom resolution type \"{resolution_type}\". Custom resolutions must be handled by custom fetchers."
    )]
    #[diagnostic(code(ERR_PNPM_UNSUPPORTED_RESOLUTION_TYPE))]
    UnsupportedResolutionType { resolution_type: String },

    /// No variant in a [`LockfileResolution::Variations`](pnpm_lockfile::LockfileResolution::Variations) matches the
    /// selected triple `(os, cpu, libc?)`. Surfaces with that triple
    /// plus the list of advertised target triples so the user can see
    /// at a glance whether they're running on an unsupported platform
    /// or whether the lockfile was generated without the host's
    /// architecture in mind.
    #[display(
        "Package `{package_key}` is a runtime dependency, but none of its declared variants matches the selected triple ({selected_target}). Available variants: {available_targets}"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_NO_MATCHING_PLATFORM_VARIANT))]
    NoMatchingPlatformVariant {
        package_key: String,
        selected_target: String,
        /// Pre-rendered list of the lockfile's advertised target
        /// triples, formatted as `os/cpu[+libc]`. Lives in the error
        /// payload rather than the lockfile (which is borrowed from
        /// the install request) so the error stays cheap to construct
        /// at the rejection site and isn't tied to the lockfile's
        /// lifetime.
        available_targets: String,
    },

    /// A variant inside a [`LockfileResolution::Variations`](pnpm_lockfile::LockfileResolution::Variations) carries
    /// a resolution other than [`LockfileResolution::Binary`](pnpm_lockfile::LockfileResolution::Binary).
    /// The lockfile contract guarantees variants are atomic
    /// `BinaryResolution`s; this variant catches lockfile corruption
    /// or a future shape pacquet doesn't recognise rather than
    /// silently routing through and confusing the install pipeline.
    #[display(
        "Package `{package_key}` carries a runtime variant whose inner resolution is `{inner_kind}` rather than `binary`; pnpm only knows how to install binary-shaped variants."
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_VARIANT_HAS_NON_BINARY_RESOLUTION))]
    VariantHasNonBinaryResolution { package_key: String, inner_kind: &'static str },

    /// Serializing the synthesized runtime `package.json` failed.
    /// The manifest is a small fixed-shape JSON object (`name`,
    /// `version`, `bin`); `serde_json` rejects this only on a
    /// numeric or struct value the writer can't render, which can't
    /// happen for the three string-typed fields we pass it.
    /// Surfaces as a typed error rather than a panic so a future
    /// shape change to [`BinarySpec`](pnpm_lockfile::BinarySpec) doesn't crash an install.
    #[display(
        "Failed to serialize the synthesized package.json for runtime entry `{package_key}`: {error}"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_SYNTHESIZE_RUNTIME_MANIFEST))]
    SynthesizeRuntimeManifest {
        package_key: String,
        #[error(source)]
        error: serde_json::Error,
    },
}

/// What installing one package produced.
#[derive(Debug)]
pub struct InstalledPackage {
    pub cas_paths: HashMap<String, PathBuf>,
    /// Whether [`Self::cas_paths`] points at mutable local source
    /// rather than immutable content-addressed entries. See
    /// [`crate::CreateVirtualDirBySnapshot::source_is_mutable`].
    pub source_is_mutable: bool,
}

#[cfg(test)]
mod tests;
