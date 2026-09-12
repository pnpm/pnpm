//! The options [`resolve_importer`](super::resolve_importer) takes, and
//! the hand-written `Debug` that prints the hook slots as placeholders
//! rather than their closures.

use crate::{ManifestHook, resolve_importer::DependencyOverrider};
use chrono::{DateTime, Utc};
use pnpm_catalogs_types::Catalogs;
use pnpm_patching::PatchGroupRecord;
use pnpm_resolving_resolver_base::{PreferredVersions, ResolveOptions};
use std::{path::PathBuf, sync::Arc};

/// Options threaded into [`fn@resolve_importer`].
pub struct ResolveImporterOptions {
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
    /// single-importer [`fn@resolve_importer`] path supplies its own.
    pub resolve_peers_from_workspace_root: bool,

    /// Threaded into [`ResolvePeersOptions::dedupe_peers`] on every
    /// `resolve_peers` invocation inside the auto-install-peers loop.
    /// See the field doc on [`ResolvePeersOptions`] for the behavior.
    pub dedupe_peers: bool,

    /// The `dedupePeerDependents` setting (default `true`). Together
    /// with [`Self::auto_install_peers`] it decides whether the hoist
    /// rounds run at all — a peer nothing asked to install is still
    /// hoisted to collapse peer-suffixed variants, so turning both off
    /// leaves every missing peer missing. The cross-importer collapse
    /// itself lives in [`fn@crate::resolve_peers_workspace`] and reads
    /// the setting separately.
    pub dedupe_peer_dependents: bool,

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

    pub base_opts: ResolveOptions,

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
    pub lockfile_dir: Option<PathBuf>,

    /// Absolute path of the importer's `node_modules` directory.
    /// Forwarded to [`crate::resolve_peers()`] for the
    /// `excludeLinksFromLockfile` remap; the gate is no-op when `None`.
    pub modules_dir: Option<PathBuf>,

    /// Cap on the rendered peer-suffix before the suffix is replaced
    /// with a short hash. Threaded into [`fn@resolve_peers`] via
    /// [`ResolvePeersOptions`]. This is the `peersSuffixMaxLength`
    /// setting (default 1000).
    pub peers_suffix_max_length: usize,

    pub catalog_server: bool,

    /// `readPackageHook` applied to every resolved manifest before
    /// downstream consumers see it. Today drives `packageExtensions`;
    /// see [`crate::ManifestHook`].
    pub manifest_hook: Option<ManifestHook>,

    /// Post-pnpmfile manifest hook (overrides). See
    /// `WorkspaceTreeCtx::overrides_hook` for the ordering contract.
    pub overrides_hook: Option<ManifestHook>,

    /// `pnpmfileHook` applied to every resolved manifest. Wraps
    /// `readPackage` from `.pnpmfile.cjs` / `pnpmfile.cjs`.
    pub pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
}

impl std::fmt::Debug for ResolveImporterOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveImporterOptions")
            .field("auto_install_peers", &self.auto_install_peers)
            .field(
                "auto_install_peers_from_highest_match",
                &self.auto_install_peers_from_highest_match,
            )
            .field("resolve_peers_from_workspace_root", &self.resolve_peers_from_workspace_root)
            .field("dedupe_peers", &self.dedupe_peers)
            .field("dedupe_peer_dependents", &self.dedupe_peer_dependents)
            .field("all_preferred_versions", &self.all_preferred_versions)
            .field(
                "override_bare_specifier",
                &self.override_bare_specifier.as_ref().map(|_| "<overrider>"),
            )
            .field("patched_dependencies", &self.patched_dependencies)
            .field("base_opts", &self.base_opts)
            .field("pick_lowest_direct", &self.pick_lowest_direct)
            .field("subdep_published_by", &self.subdep_published_by)
            .field("catalogs", &self.catalogs)
            .field("exclude_links_from_lockfile", &self.exclude_links_from_lockfile)
            .field("lockfile_dir", &self.lockfile_dir)
            .field("modules_dir", &self.modules_dir)
            .field("peers_suffix_max_length", &self.peers_suffix_max_length)
            .field("catalog_server", &self.catalog_server)
            .field("manifest_hook", &self.manifest_hook.as_ref().map(|_| "<hook>"))
            .field("overrides_hook", &self.overrides_hook.as_ref().map(|_| "<hook>"))
            .field("pnpmfile_hook", &self.pnpmfile_hook.as_ref().map(|_| "<hook>"))
            .finish()
    }
}
