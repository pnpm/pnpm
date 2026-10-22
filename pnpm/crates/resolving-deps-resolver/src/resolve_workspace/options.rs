//! The options [`fn@super::resolve_workspace`] takes, and the split that
//! hands most of them to the shared tree context.

use crate::resolve_dependency_tree::{UpdateDepth, UpdateReuseScope, WorkspaceTreeCtx};
use pnpm_lockfile::RegistryContext;
use pnpm_package_manifest::PackageManifest;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

/// One importer's input to [`fn@super::resolve_workspace`].
pub struct WorkspaceImporter<'a> {
    pub id: String,
    pub manifest: &'a PackageManifest,
}

/// Workspace-shared opts that don't vary per importer.
pub struct WorkspaceResolveOptions {
    /// Whether named workspace resolutions may be shared across importers.
    /// When `true`, an eligible named `workspace:` request resolves once
    /// against a cache key that omits the consuming importer's `project_dir`,
    /// and the importer-relative `link:` is rendered from that canonical
    /// result afterwards. Must stay `false` whenever the resolver chain can
    /// make a resolution depend on the consuming importer beyond that
    /// rendering — a pnpmfile custom resolver above all.
    pub share_workspace_resolutions: bool,
    /// Package-name → semver-range map from the
    /// `pnpm.allowedDeprecatedVersions` setting. When a newly-resolved
    /// package is deprecated and its `name@version` satisfies an entry
    /// here, the deprecation warning is suppressed.
    pub allowed_deprecated_versions: BTreeMap<String, String>,
    /// How a package's registry is decided and what it serves: the scope
    /// map, the named-registry aliases (built-ins merged with the user's
    /// setting), and the per-registry settings. Used to materialize a
    /// prior `Registry` lockfile resolution back into its tarball URL when
    /// building the `currentPkg` payload custom resolvers receive.
    pub registry_context: RegistryContext,
    pub peers: WorkspacePeerResolutionOptions,
    pub hooks: WorkspaceResolveHooks,
    pub reuse: WorkspaceLockfileReuse,
    pub version: WorkspaceVersionResolution,
}

#[derive(Default)]
pub struct WorkspacePeerResolutionOptions {
    pub dedupe_peers: bool,
    /// Threaded into [`crate::WorkspacePeerSettings::dedupe_injected_deps`].
    pub dedupe_injected_deps: bool,
    /// Threaded into [`crate::WorkspacePeerSettings::dedupe_peer_dependents`],
    /// and overrides every per-importer
    /// [`crate::ImporterPeerOptions::dedupe_peer_dependents`].
    pub dedupe_peer_dependents: bool,
    /// Threaded into
    /// [`crate::WorkspacePeerSettings::resolve_peers_from_workspace_root`].
    pub resolve_peers_from_workspace_root: bool,
    /// Threaded into [`crate::PeerLinkOptions::exclude_links_from_lockfile`]
    /// for the workspace-wide peer pass. Per-importer
    /// [`crate::PeerLinkOptions::modules_dir`] comes from each
    /// [`crate::ImporterPeerInput::modules_dir`].
    pub exclude_links_from_lockfile: bool,
    pub lockfile_dir: PathBuf,
    pub peers_suffix_max_length: usize,
    /// The install's `autoInstallPeers` setting, threaded onto the
    /// shared [`WorkspaceTreeCtx`] so the tree walk drops
    /// peer-shadowed `dependencies` entries. Also overrides every
    /// per-importer
    /// [`crate::ImporterPeerOptions::auto_install_peers`] — the
    /// setting is workspace-wide.
    pub auto_install_peers: bool,
}

#[derive(Default)]
pub struct WorkspaceResolveHooks {
    /// `context.log(...)` sink for the `pnpmfile_hook`'s `readPackage`
    /// calls, pre-bound to the install's reporter. `None` leaves hook
    /// logging a no-op.
    pub read_package_log: Option<pnpm_hooks::LogFn>,
    /// Sink for skipped-optional-dependency notifications, pre-bound to
    /// the install's reporter (the install layer forwards each one as a
    /// `pnpm:skipped-optional-dependency` `resolution_failure` debug
    /// log). `None` keeps the skip behavior but drops the notification.
    pub skipped_optional_log: Option<crate::SkippedOptionalLogFn>,
    /// Sink told about every package whose subtree has settled peer-free,
    /// so the install layer can materialize it into the virtual store
    /// before peer resolution. `None` skips the sweep. See
    /// [`crate::FinalizedPackageFn`].
    pub finalized_package: Option<crate::FinalizedPackageFn>,
    /// Sink for deprecation notifications, pre-bound to the install's
    /// reporter (the install layer forwards each one as a
    /// `pnpm:deprecation` debug log). `None` keeps the deprecation
    /// check but drops the notification.
    pub deprecation_log: Option<crate::DeprecationLogFn>,
    pub manifests: crate::ManifestTransformHooks,
}

#[derive(smart_default::SmartDefault)]
pub struct WorkspaceLockfileReuse {
    /// The prior `pnpm-lock.yaml` the install started from, when one
    /// exists. Threaded into [`WorkspaceTreeCtx`] so the tree walk can
    /// reuse already-resolved dependencies instead of re-resolving them
    /// (see `pnpm/plans/LOCKFILE_RESOLUTION_REUSE.md`). `None` on a
    /// first install or when reuse is disabled.
    pub lockfile: Option<Arc<pnpm_lockfile::Lockfile>>,
    /// Whether the walk may reuse whole already-resolved subtrees from
    /// [`Self::lockfile`]. `false` keeps the lockfile as a
    /// per-edge version-pin source only: every node re-resolves against
    /// its (hook-rewritten) manifest range, and an edge whose recorded
    /// version still satisfies that range stays on it — mirroring the
    /// TypeScript resolver's forced full resolution, which forces the
    /// walk without unpinning still-satisfied edges. The config drift
    /// that denied subtree reuse stays effective: hooks rewrite the
    /// drifted manifests before the satisfies check, so the edges a
    /// changed override or extension reaches re-resolve.
    #[default(true)]
    pub subtrees: bool,
    /// Which dependencies `pacquet update` excludes from lockfile-
    /// resolution reuse. [`UpdateReuseScope::All`] for `install` / `add`.
    pub scope: UpdateReuseScope,
    /// Per-importer update scopes for filtered workspace updates. An importer
    /// absent from this map uses [`Self::scope`].
    pub scopes_by_importer: BTreeMap<String, UpdateReuseScope>,
    /// `pacquet update --depth`: how deep the update reaches. Nodes
    /// past the ceiling keep their locked resolutions even when their
    /// name is an update target.
    pub depth: UpdateDepth,
    /// Reconsider these packages using the existing-version preferences,
    /// without treating them as explicit update targets.
    pub dedupe: crate::UpdateTargets,
}

#[derive(Default)]
pub struct WorkspaceVersionResolution {
    /// When `true`, every importer's direct dependencies are resolved
    /// to their lowest satisfying version (`resolutionMode: time-based`
    /// / `lowest-direct`). Threaded onto each
    /// [`crate::ImporterResolutionInputs::pick_lowest_direct`].
    pub pick_lowest_direct: bool,
    /// When `true` (`resolutionMode: time-based`), a pre-pass resolves
    /// every importer's direct deps to find the newest publication
    /// date, then constrains all transitive deps to versions published
    /// no later than that (plus a one-hour delta), clamped by any
    /// `minimumReleaseAge` cutoff.
    pub time_based: bool,
}

/// What the pass keeps for itself once the shared tree context has
/// taken the hooks, logs and lockfile.
pub(super) struct PassSettings {
    /// The lockfile's recorded publish dates, taken only for the
    /// `time-based` pre-pass that reads them — a lockfile is untrusted
    /// input, so an install that will not consult the dates must not
    /// copy them.
    pub(super) recorded_time: Option<BTreeMap<String, String>>,
    pub(super) peers: WorkspacePeerResolutionOptions,
    pub(super) version: WorkspaceVersionResolution,
}

impl WorkspaceResolveOptions {
    pub(super) fn split(self) -> (Arc<WorkspaceTreeCtx>, PassSettings) {
        let recorded_time = self.version.time_based
            .then(|| {
                self.reuse.lockfile.as_ref().and_then(|lockfile| lockfile.time.clone())
            })
            .flatten();
        let settings = PassSettings { recorded_time, peers: self.peers, version: self.version };
        let workspace = WorkspaceTreeCtx::default()
            .with_shared_workspace_resolutions(self.share_workspace_resolutions)
            .with_hooks(self.hooks)
            .with_lockfile_reuse(self.reuse)
            .with_allowed_deprecated_versions(self.allowed_deprecated_versions)
            .with_auto_install_peers(settings.peers.auto_install_peers)
            .with_registry_context(self.registry_context);
        (Arc::new(workspace), settings)
    }
}
