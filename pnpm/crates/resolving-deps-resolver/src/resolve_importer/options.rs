use crate::{
    DependencyOverrider, ResolveDependencyTreeError, ResolvePeersResult, ResolvedTree,
    resolve_dependency_tree::{TreeCtx, WorkspaceTreeCtx},
};
use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::PackageManifestError;
use pnpm_patching::PatchGroupRecord;
use pnpm_resolving_resolver_base::{PreferredVersions, ResolveOptions};
use std::sync::Arc;

/// Options threaded into [`fn@crate::resolve_importer`].
pub struct ResolveImporterOptions {
    pub base_opts: ResolveOptions,
    /// Cap on the rendered peer-suffix before the suffix is replaced
    /// with a short hash. Threaded into [`fn@crate::resolve_peers`] via
    /// [`crate::ResolvePeersOptions`]. This is the `peersSuffixMaxLength`
    /// setting (default 1000).
    pub peers_suffix_max_length: usize,
    pub peers: ImporterPeerOptions,
    pub links: PeerLinkOptions,
    pub resolution: ImporterResolutionInputs,
    pub hooks: ManifestTransformHooks,
}

#[derive(Debug, Clone)]
pub struct ImporterPeerOptions {
    /// When true, missing required peers get installed at the importer
    /// even if no preferred version is in scope (the picker uses the
    /// peer's declared range as the specifier).
    pub auto_install_peers: bool,
    /// When true, conflicting peer ranges from multiple consumers are
    /// merged with `||` instead of being dropped on intersection
    /// failure. This is the `autoInstallPeersFromHighestMatch` setting.
    pub auto_install_peers_from_highest_match: bool,
    /// When true, a missing peer matching one of the *workspace root*
    /// importer's direct deps is installed from that dep's specifier.
    /// [`fn@crate::resolve_workspace`] supplies the root's deps; the
    /// single-importer [`fn@crate::resolve_importer`] path supplies its own.
    pub resolve_peers_from_workspace_root: bool,
    /// Threaded into [`crate::ResolvePeersOptions::dedupe_peers`] on every
    /// `resolve_peers` invocation inside the auto-install-peers loop.
    pub dedupe_peers: bool,
    /// The `dedupePeerDependents` setting (default `true`). Together
    /// with [`Self::auto_install_peers`] it decides whether the hoist
    /// rounds run at all — a peer nothing asked to install is still
    /// hoisted to collapse peer-suffixed variants, so turning both off
    /// leaves every missing peer missing. The cross-importer collapse
    /// itself lives in [`fn@crate::resolve_peers_workspace`] and reads
    /// the setting separately.
    pub dedupe_peer_dependents: bool,
}

#[derive(Debug, Default, Clone)]
pub struct PeerLinkOptions {
    /// When `true`, `link:` direct deps whose target lives outside
    /// the lockfile root are seeded into the peer-resolution parent
    /// map with a remapped node id
    /// (`link:<rel-from-lockfile_dir-to-modules_dir>/<alias>`) so the
    /// peer suffix stays stable across machines. This is the
    /// `excludeLinksFromLockfile` flow. The remap fires only when
    /// [`Self::lockfile_dir`] and [`Self::modules_dir`] are both set.
    pub exclude_links_from_lockfile: bool,
    /// Absolute path of the directory `pnpm-lock.yaml` lives in.
    /// Forwarded to [`crate::resolve_peers()`] for the
    /// `excludeLinksFromLockfile` remap; the gate is no-op when `None`.
    pub lockfile_dir: Option<std::path::PathBuf>,
    /// Absolute path of the importer's `node_modules` directory.
    /// Forwarded to [`crate::resolve_peers()`] for the
    /// `excludeLinksFromLockfile` remap; the gate is no-op when `None`.
    pub modules_dir: Option<std::path::PathBuf>,
}

pub struct ImporterResolutionInputs {
    /// Seed for the preferred-versions tie-break table: the lockfile +
    /// manifest entries the peer-hoist pickers bias toward, so a
    /// version a sibling already brought is reused instead of adding a
    /// second instance. Versions resolved into the settled tree are
    /// derived once, workspace-wide, on the tree context and merged
    /// with these seed buckets per lookup (seed entries win) — see
    /// `TreeCtx::preferred_versions_for_names`. Pass the result of
    /// `get_preferred_versions_from_lockfile_and_manifests` from the
    /// `lockfile-preferred-versions` crate, or an empty map when no
    /// lockfile + manifest seeding is available.
    pub all_preferred_versions: Arc<PreferredVersions>,
    /// Applies `overrides` to auto-installed peers. See
    /// [`crate::DependencyOverrider`].
    pub override_bare_specifier: Option<Arc<DependencyOverrider>>,
    /// Configured `patchedDependencies`, grouped by package name. The
    /// tree walker appends `(patch_hash=<hash>)` to each matched
    /// package's `pkgIdWithPatchHash` and records the matched key on
    /// [`crate::ResolvedTree::applied_patches`]. `None` when no
    /// patches are configured for this install.
    pub patched_dependencies: Option<Arc<PatchGroupRecord>>,
    /// When `true`, the importer's direct dependencies are resolved to
    /// their lowest satisfying version (`resolutionMode: time-based` /
    /// `lowest-direct`). Transitive deps are always picked highest.
    pub pick_lowest_direct: bool,
    /// Publish-date cutoff applied to transitive dependencies. Under
    /// `resolutionMode: time-based` this is the workspace-wide cutoff
    /// derived from the resolved direct deps (the multi-importer
    /// orchestrator [`fn@crate::resolve_workspace`] computes it and
    /// overrides this field); otherwise it should equal
    /// `base_opts.published_by` (the `minimumReleaseAge` cutoff) so
    /// subdep resolution is unchanged. Direct deps always use
    /// `base_opts.published_by`, never this value.
    pub subdep_published_by: Option<DateTime<Utc>>,
    /// Catalogs parsed from `pnpm-workspace.yaml`. Applied to importer
    /// dependencies and to children of injected workspace packages.
    pub catalogs: Catalogs,
    /// Directory `pnpm-workspace.yaml` sits in, which a `file:` /
    /// `link:` catalog entry's relative path is measured from. `None`
    /// when the install has no workspace manifest, and so no catalogs.
    pub catalogs_dir: Option<std::path::PathBuf>,
    pub catalog_server: bool,
}

#[derive(Default, Clone)]
pub struct ManifestTransformHooks {
    /// `readPackageHook` applied to every resolved manifest before
    /// downstream consumers see it. Today drives `packageExtensions`;
    /// see [`crate::ManifestHook`].
    pub manifest_hook: Option<crate::ManifestHook>,
    /// Post-pnpmfile manifest hook (overrides). See
    /// `WorkspaceTreeCtx::overrides_hook` for the ordering contract.
    pub overrides_hook: Option<crate::ManifestHook>,
    /// `pnpmfileHook` applied to every resolved manifest. Wraps
    /// `readPackage` from `.pnpmfile.cjs` / `pnpmfile.cjs`.
    pub pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
}

impl std::fmt::Debug for ResolveImporterOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveImporterOptions")
            .field("auto_install_peers", &self.peers.auto_install_peers)
            .field(
                "auto_install_peers_from_highest_match",
                &self.peers.auto_install_peers_from_highest_match,
            )
            .field(
                "resolve_peers_from_workspace_root",
                &self.peers.resolve_peers_from_workspace_root,
            )
            .field("dedupe_peers", &self.peers.dedupe_peers)
            .field("dedupe_peer_dependents", &self.peers.dedupe_peer_dependents)
            .field("all_preferred_versions", &self.resolution.all_preferred_versions)
            .field(
                "override_bare_specifier",
                &self.resolution.override_bare_specifier.as_ref().map(|_| "<overrider>"),
            )
            .field("patched_dependencies", &self.resolution.patched_dependencies)
            .field("base_opts", &self.base_opts)
            .field("pick_lowest_direct", &self.resolution.pick_lowest_direct)
            .field("subdep_published_by", &self.resolution.subdep_published_by)
            .field("catalogs", &self.resolution.catalogs)
            .field("exclude_links_from_lockfile", &self.links.exclude_links_from_lockfile)
            .field("lockfile_dir", &self.links.lockfile_dir)
            .field("modules_dir", &self.links.modules_dir)
            .field("peers_suffix_max_length", &self.peers_suffix_max_length)
            .field("catalog_server", &self.resolution.catalog_server)
            .field("manifest_hook", &self.hooks.manifest_hook.as_ref().map(|_| "<hook>"))
            .field("overrides_hook", &self.hooks.overrides_hook.as_ref().map(|_| "<hook>"))
            .field("pnpmfile_hook", &self.hooks.pnpmfile_hook.as_ref().map(|_| "<hook>"))
            .finish()
    }
}

pub(crate) struct HoistSettings {
    pub(crate) all_preferred_versions: Arc<PreferredVersions>,
    pub(crate) override_bare_specifier: Option<Arc<DependencyOverrider>>,
    pub(crate) project_dir: std::path::PathBuf,
    pub(crate) peers_suffix_max_length: usize,
    pub(crate) peers: ImporterPeerOptions,
    pub(crate) links: PeerLinkOptions,
}

impl ResolveImporterOptions {
    pub(crate) fn into_tree_ctx(
        self,
        importer_id: &str,
        importer_order: usize,
        workspace: Arc<WorkspaceTreeCtx>,
    ) -> (TreeCtx, HoistSettings) {
        let project_dir = self.base_opts.project.project_dir.clone();
        let tree_lockfile_dir =
            self.links.lockfile_dir.clone().unwrap_or_else(|| project_dir.clone());
        let ctx = TreeCtx::with_workspace(workspace, self.base_opts)
            .with_lockfile_dir(&tree_lockfile_dir)
            .with_importer_id(importer_id)
            .with_importer_order(importer_order)
            .with_patched_dependencies(self.resolution.patched_dependencies)
            .with_resolution_mode(
                self.resolution.pick_lowest_direct,
                self.resolution.subdep_published_by,
            )
            .with_catalogs(self.resolution.catalogs, self.resolution.catalogs_dir.clone());
        let settings = HoistSettings {
            all_preferred_versions: self.resolution.all_preferred_versions,
            override_bare_specifier: self.resolution.override_bare_specifier,
            project_dir,
            peers_suffix_max_length: self.peers_suffix_max_length,
            peers: self.peers,
            links: self.links,
        };
        (ctx, settings)
    }
}

/// Result of [`fn@crate::resolve_importer`] — the fully-walked tree plus the
/// peer-resolution output the install layer consumes.
#[derive(Debug)]
pub struct ResolveImporterResult {
    pub resolved_tree: ResolvedTree,
    pub peers_result: ResolvePeersResult,
}

/// Error envelope for [`fn@crate::resolve_importer`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveImporterError {
    Resolve(#[error(source)] ResolveDependencyTreeError),

    /// Reading the manifest of a workspace-root `link:` / `file:`
    /// dependency, whose version stands in for the peer it may satisfy.
    RootDepManifest(#[error(source)] PackageManifestError),
}

impl From<ResolveDependencyTreeError> for ResolveImporterError {
    fn from(err: ResolveDependencyTreeError) -> Self {
        ResolveImporterError::Resolve(err)
    }
}

impl From<PackageManifestError> for ResolveImporterError {
    fn from(err: PackageManifestError) -> Self {
        ResolveImporterError::RootDepManifest(err)
    }
}
