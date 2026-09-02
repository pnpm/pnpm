//! The per-importer half of the walk: [`TreeCtx`], its builders, and
//! the per-edge [`ResolveOptions`] derivations — the depth-specific
//! pick, the project-relative cache scope, and the `file:` specifier
//! resolved against its declaring manifest's directory.

use chrono::{DateTime, Utc};
use pnpm_catalogs_types::Catalogs;
use pnpm_hooks::PnpmfileHooks;
use pnpm_patching::PatchGroupRecord;
use pnpm_resolving_resolver_base::{ResolveOptions, WantedDependency};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::resolved_tree::{DirectDep, ResolvedTree};

use super::{ManifestHook, UpdateReuseScope, reuse::UpdateScope, workspace_ctx::WorkspaceTreeCtx};

/// Whether a wanted dep's resolution is computed relative to the
/// consuming importer's directory rather than being
/// importer-independent. True for the `link:` / `file:` / `workspace:`
/// protocols, whose resolved path `resolve_from_local_package` derives
/// from `project_dir`. Such resolutions must not be shared across
/// importers in [`WantedKey`].
///
/// [`WantedKey`]: super::workspace_ctx::WantedKey
pub(super) fn project_relative_cache_scope(
    wanted: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<super::workspace_ctx::PathKey> {
    (wanted.bare_specifier.as_deref().is_some_and(|spec| {
        spec.starts_with("link:") || spec.starts_with("file:") || spec.starts_with("workspace:")
    }) || (opts.always_try_workspace_packages && opts.workspace_packages.is_some()))
    .then(|| opts.project_dir.clone().into())
}

/// The directory the `file:` dependencies declared by a resolved
/// package resolve against — pnpm's `parentPkg.rootDir`. A package
/// copied from a local directory declares them relative to its own
/// directory; every other resolution has no directory of its own, so
/// its `file:` children stay on the consuming importer's.
///
/// Only `file:`-shaped directory resolutions qualify. They record
/// their directory relative to the lockfile root (absolute under
/// `preserveAbsolutePaths`), whereas a `link:`-shaped one records it
/// relative to the consuming importer (workspace links) or absolute
/// (the local resolver). Nothing asks for a linked node's directory:
/// a linked project resolves its own dependencies as a separate
/// importer, so the walk never descends into one.
pub(super) fn declaring_manifest_dir(
    ctx: &TreeCtx,
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> Option<Arc<Path>> {
    let pnpm_lockfile::LockfileResolution::Directory(resolution) = &result.resolution else {
        return None;
    };
    if !result.id.as_str().starts_with("file:") {
        return None;
    }
    let directory = Path::new(&resolution.directory);
    let absolute = if directory.is_absolute() {
        pnpm_fs::lexical_normalize(directory)
    } else {
        pnpm_fs::lexical_normalize(&ctx.lockfile_dir.join(directory))
    };
    Some(Arc::from(absolute))
}

/// Point a `file:` specifier at the directory of the manifest that
/// declares it ([`declaring_manifest_dir`]) instead of the consuming
/// importer's. Every other specifier keeps the caller's options.
pub(super) fn opts_relative_to_declaring_manifest<'a>(
    opts: &'a ResolveOptions,
    wanted: &WantedDependency,
    parent_dir: Option<&Path>,
) -> Cow<'a, ResolveOptions> {
    match parent_dir {
        Some(parent_dir)
            if wanted.bare_specifier.as_deref().is_some_and(|spec| spec.starts_with("file:")) =>
        {
            Cow::Owned(ResolveOptions { project_dir: parent_dir.to_path_buf(), ..opts.clone() })
        }
        _ => Cow::Borrowed(opts),
    }
}

/// Mutable workspace for an in-flight tree walk. The orchestrator
/// (`resolve_importer`) holds one of these across hoist iterations and
/// extends it via [`extend_tree`] so newly-hoisted peer dependencies
/// reuse the existing per-id dedup map instead of restarting the walk.
///
/// The shared per-`pkgIdWithPatchHash` dedup maps live on
/// [`WorkspaceTreeCtx`] behind an `Arc`. In single-importer mode this
/// `Arc` is sole-owned by [`TreeCtx`]; in multi-importer mode
/// `Arc::clone(&workspace)` is handed to every per-importer
/// [`TreeCtx`] so importer N's tree walk reuses importer M's resolved
/// envelopes via the shared maps.
///
/// [`extend_tree`]: super::extend_tree
pub struct TreeCtx {
    pub(super) base_opts: ResolveOptions,
    /// Absolute root used to make ownerless snapshot `link:` ids stable
    /// across importers at different depths.
    pub(super) lockfile_dir: PathBuf,
    /// [`ResolveOptions`] handed to the resolver for importer-level
    /// (direct) dependencies — `depth == 0`. Differs from `base_opts`
    /// only in `pick_lowest_version`, which is set under
    /// `resolutionMode: time-based` / `lowest-direct`. Built once per
    /// importer by [`Self::with_resolution_mode`].
    direct_opts: ResolveOptions,
    /// [`ResolveOptions`] handed to the resolver for transitive
    /// dependencies — `depth > 0`. Always picks highest; carries the
    /// `time-based` publish-date cutoff in `published_by`. Built once
    /// per importer by [`Self::with_resolution_mode`].
    subdep_opts: ResolveOptions,
    /// Workspace catalogs used to resolve `catalog:` children of injected
    /// workspace packages. Other transitive dependencies keep catalog
    /// resolution disabled.
    pub(super) catalogs: Catalogs,
    pub(super) workspace: Arc<WorkspaceTreeCtx>,
    /// Configured `patchedDependencies` (already grouped by name).
    /// Shared by `Arc` so the lookup table doesn't get cloned per
    /// recursive call. `None` when no patches are configured for this
    /// install.
    pub(super) patched_dependencies: Option<Arc<PatchGroupRecord>>,
    /// The importer this per-importer context walks for. Recorded into
    /// [`WorkspaceTreeCtx`]'s `first_importer_by_pkg` when one of its
    /// occurrences owns a package's shared children context.
    pub(super) importer_id: String,
    pub(super) importer_order: usize,
    /// The importer-wide slice of the shared workspace-resolution cache
    /// key, built once here so the per-edge key construction shares it.
    /// See [`super::workspace_ctx::WorkspaceResolutionOptionsKey`].
    pub(super) workspace_resolution_options_key:
        Arc<super::workspace_ctx::WorkspaceResolutionOptionsKey>,
    /// Importer anchor for re-rendering `link:` targets against
    /// [`Self::lockfile_dir`], derived once here — the per-edge
    /// `pkgIdWithPatchHash` fold reads it for every workspace edge.
    pub(super) link_anchor: crate::link_target::ImporterAnchor,
    /// Like [`Self::link_anchor`], but against `base_opts.lockfile_dir`
    /// verbatim — the pair the per-edge canonical-resolution rendering
    /// computed from, kept separate so the cache changes no behavior
    /// even when the two lockfile-dir spellings differ.
    pub(super) base_link_anchor: crate::link_target::ImporterAnchor,
}

impl TreeCtx {
    /// Construct a single-importer context with a fresh
    /// [`WorkspaceTreeCtx`]. The multi-importer orchestrator uses
    /// [`Self::with_workspace`] instead so per-importer contexts share
    /// the same workspace ctx.
    #[must_use]
    pub fn new(base_opts: ResolveOptions) -> Self {
        let lockfile_dir = if base_opts.lockfile_dir.as_os_str().is_empty() {
            base_opts.project_dir.clone()
        } else {
            base_opts.lockfile_dir.clone()
        };
        let lockfile_dir = pnpm_fs::lexical_normalize(&lockfile_dir);
        TreeCtx {
            direct_opts: base_opts.clone(),
            subdep_opts: base_opts.clone(),
            workspace_resolution_options_key: Arc::new(
                super::workspace_ctx::WorkspaceResolutionOptionsKey::new(&base_opts),
            ),
            link_anchor: crate::link_target::ImporterAnchor::new(
                &base_opts.project_dir,
                &lockfile_dir,
            ),
            base_link_anchor: crate::link_target::ImporterAnchor::new(
                &base_opts.project_dir,
                &base_opts.lockfile_dir,
            ),
            base_opts,
            lockfile_dir,
            catalogs: Catalogs::new(),
            workspace: Arc::new(WorkspaceTreeCtx::default()),
            patched_dependencies: None,
            importer_id: pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY.to_string(),
            importer_order: 0,
        }
    }

    /// Construct a per-importer context that shares its dedup maps
    /// with `workspace`. The caller is responsible for keeping
    /// `workspace` alive across importers (typically via
    /// `Arc::clone(&workspace)`).
    pub fn with_workspace(workspace: Arc<WorkspaceTreeCtx>, base_opts: ResolveOptions) -> Self {
        let lockfile_dir = if base_opts.lockfile_dir.as_os_str().is_empty() {
            base_opts.project_dir.clone()
        } else {
            base_opts.lockfile_dir.clone()
        };
        let lockfile_dir = pnpm_fs::lexical_normalize(&lockfile_dir);
        TreeCtx {
            direct_opts: base_opts.clone(),
            subdep_opts: base_opts.clone(),
            workspace_resolution_options_key: Arc::new(
                super::workspace_ctx::WorkspaceResolutionOptionsKey::new(&base_opts),
            ),
            link_anchor: crate::link_target::ImporterAnchor::new(
                &base_opts.project_dir,
                &lockfile_dir,
            ),
            base_link_anchor: crate::link_target::ImporterAnchor::new(
                &base_opts.project_dir,
                &base_opts.lockfile_dir,
            ),
            base_opts,
            lockfile_dir,
            catalogs: Catalogs::new(),
            workspace,
            patched_dependencies: None,
            importer_id: pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY.to_string(),
            importer_order: 0,
        }
    }

    #[must_use]
    pub(crate) fn with_lockfile_dir(mut self, lockfile_dir: &Path) -> Self {
        self.lockfile_dir = pnpm_fs::lexical_normalize(lockfile_dir);
        self.link_anchor = crate::link_target::ImporterAnchor::new(
            &self.base_opts.project_dir,
            &self.lockfile_dir,
        );
        self
    }

    /// Derive the depth-specific resolve options from `resolutionMode`.
    ///
    /// - `pick_lowest_direct` — resolve direct dependencies to their
    ///   lowest satisfying version (`time-based` / `lowest-direct`).
    /// - `subdep_published_by` — the publish-date cutoff applied to
    ///   transitive dependencies. Under `time-based` this is the
    ///   workspace-wide cutoff computed from the resolved direct deps
    ///   (clamped by `minimumReleaseAge`); otherwise it is just
    ///   `base_opts.published_by` (the `minimumReleaseAge` cutoff),
    ///   leaving subdep resolution unchanged.
    ///
    /// Splits the importer-dep pick from the subdep pick (always
    /// highest, constrained by the computed `publishedBy`).
    #[must_use]
    pub fn with_resolution_mode(
        mut self,
        pick_lowest_direct: bool,
        subdep_published_by: Option<DateTime<Utc>>,
    ) -> Self {
        self.direct_opts.pick_lowest_version = pick_lowest_direct;
        self.subdep_opts.pick_lowest_version = false;
        self.subdep_opts.published_by = subdep_published_by;
        self
    }

    #[must_use]
    pub fn with_catalogs(mut self, catalogs: Catalogs) -> Self {
        self.catalogs = catalogs;
        self
    }

    /// Resolve the depth-0 walks that follow with the subdep options.
    ///
    /// The importer orchestrator calls this once the manifest-declared
    /// direct deps have seeded: every later [`extend_tree`] on this ctx
    /// installs hoisted peers, which pnpm resolves like transitive deps
    /// (highest satisfying version, under the subdep publish-date
    /// cutoff) even though they land at the importer level — a hoisted
    /// peer is not a dependency the user declared, so the direct-dep
    /// pick of `resolutionMode: lowest-direct` / `time-based` must not
    /// apply to it.
    ///
    /// [`extend_tree`]: super::extend_tree
    pub fn resolve_new_direct_deps_as_subdeps(&mut self) {
        self.direct_opts = self.subdep_opts.clone();
    }

    /// The [`ResolveOptions`] to hand the resolver for a node at the
    /// given `depth`: importer-level deps (`depth == 0`) use
    /// [`Self::direct_opts`]; everything below uses
    /// [`Self::subdep_opts`].
    pub(super) fn opts_for_depth(&self, depth: i32) -> &ResolveOptions {
        if depth == 0 { &self.direct_opts } else { &self.subdep_opts }
    }

    /// Borrow the shared workspace ctx so callers can hand the same
    /// `Arc::clone` to the next per-importer [`TreeCtx`].
    #[must_use]
    pub fn workspace(&self) -> &Arc<WorkspaceTreeCtx> {
        &self.workspace
    }

    pub(super) fn update_reuse_scope(&self) -> &UpdateReuseScope {
        self.workspace.update_reuse_scope_for(&self.importer_id)
    }

    pub(super) fn update_scope(&self) -> UpdateScope<'_> {
        UpdateScope { reuse: self.update_reuse_scope(), max_depth: self.workspace.update_depth }
    }

    pub(super) fn update_cache_scope(&self) -> Option<String> {
        (!matches!(self.update_reuse_scope(), UpdateReuseScope::All))
            .then(|| self.importer_id.clone())
    }

    /// Set the importer this context walks for. See [`TreeCtx`]'s
    /// `importer_id` field.
    #[must_use]
    pub fn with_importer_id(mut self, importer_id: &str) -> Self {
        self.importer_id = importer_id.to_string();
        self
    }

    /// Set this importer's position in the workspace input order.
    /// Child-subtree ownership uses it after depth, matching pnpm's
    /// deterministic `(depth, importer order, parent path)` tie-break.
    #[must_use]
    pub fn with_importer_order(mut self, importer_order: usize) -> Self {
        self.importer_order = importer_order;
        self
    }

    /// Attach the install's `patchedDependencies` map. When `Some`,
    /// the per-node walker looks every resolved `name@version` up via
    /// [`get_patch_info`] and appends `(patch_hash=<hash>)` to the
    /// `pkgIdWithPatchHash` on a match.
    ///
    /// [`get_patch_info`]: pnpm_patching::get_patch_info
    #[must_use]
    pub fn with_patched_dependencies(
        mut self,
        patched_dependencies: Option<Arc<PatchGroupRecord>>,
    ) -> Self {
        self.patched_dependencies = patched_dependencies;
        self
    }

    /// Attach a `readPackageHook` to the underlying [`WorkspaceTreeCtx`].
    /// `manifest_hook` is workspace-wide (one hook per install), so this
    /// passthrough relies on the workspace ctx being sole-owned —
    /// `TreeCtx::new` always satisfies that, and the multi-importer
    /// orchestrator [`fn@crate::resolve_workspace`] hands the hook in via
    /// [`WorkspaceTreeCtx::with_manifest_hook`] before sharing the
    /// `Arc`. Panics if the workspace ctx has already been cloned —
    /// callers must set the hook before sharing the context.
    #[must_use]
    pub fn with_manifest_hook(mut self, manifest_hook: Option<ManifestHook>) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect("with_manifest_hook called after the workspace ctx was shared via Arc::clone")
            .manifest_hook = manifest_hook;
        self
    }

    /// Attach the post-pnpmfile [`ManifestHook`] (overrides) to the
    /// underlying [`WorkspaceTreeCtx`]; same sole-ownership contract as
    /// [`Self::with_manifest_hook`].
    #[must_use]
    pub fn with_overrides_hook(mut self, overrides_hook: Option<ManifestHook>) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect("with_overrides_hook called after the workspace ctx was shared via Arc::clone")
            .overrides_hook = overrides_hook;
        self
    }

    #[must_use]
    pub fn with_pnpmfile_hook(mut self, pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect("with_pnpmfile_hook called after the workspace ctx was shared via Arc::clone")
            .pnpmfile_hook = pnpmfile_hook;
        self
    }

    /// Attach the `context.log(...)` sink the `pnpmfile_hook`'s
    /// `readPackage` calls forward to. Like [`Self::with_pnpmfile_hook`],
    /// this targets the underlying [`WorkspaceTreeCtx`] and panics if it
    /// has already been shared via `Arc::clone`.
    #[must_use]
    pub fn with_read_package_log(mut self, read_package_log: Option<pnpm_hooks::LogFn>) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect(
                "with_read_package_log called after the workspace ctx was shared via Arc::clone",
            )
            .read_package_log = read_package_log;
        self
    }

    /// Set the install's `autoInstallPeers` flag on the underlying
    /// [`WorkspaceTreeCtx`]. Like [`Self::with_pnpmfile_hook`], panics if
    /// it has already been shared via `Arc::clone`.
    #[must_use]
    pub fn with_auto_install_peers(mut self, auto_install_peers: bool) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect(
                "with_auto_install_peers called after the workspace ctx was shared via Arc::clone",
            )
            .auto_install_peers = auto_install_peers;
        self
    }

    /// Take ownership of `self` and emit the final [`ResolvedTree`]
    /// the peer-resolution stage consumes. The orchestrator passes its
    /// cumulative [`DirectDep`] list (initial walk + each hoist
    /// iteration's contributions) as `direct`.
    ///
    /// When the [`WorkspaceTreeCtx`] is sole-owned by this context
    /// (single-importer install) the inner mutex contents move
    /// directly into the [`ResolvedTree`]; otherwise the maps are
    /// cloned out via [`WorkspaceTreeCtx::snapshot`].
    #[must_use]
    pub fn into_resolved_tree(self, direct: Vec<DirectDep>) -> ResolvedTree {
        match Arc::try_unwrap(self.workspace) {
            Ok(ws) => ws.into_resolved_tree(direct),
            Err(arc) => arc.snapshot(direct),
        }
    }

    /// Build a snapshot of the current tree state without consuming
    /// `self`. The orchestrator's hoist loop snapshots after each
    /// [`extend_tree`] call to run [`fn@crate::resolve_peers`] over the
    /// growing tree and find missing peers to hoist next.
    ///
    /// [`extend_tree`]: super::extend_tree
    #[must_use]
    pub fn snapshot(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        self.workspace.snapshot(direct)
    }

    /// Build an importer-scoped snapshot for a peer-hoist pass.
    #[must_use]
    pub fn snapshot_reachable_from(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        self.workspace.snapshot_reachable_from(direct)
    }

    /// The preferred-version buckets for `names`: the caller's seed
    /// entries merged with every version this run has resolved into the
    /// settled reachable tree (seed entries win per selector) — see
    /// [`WorkspaceTreeCtx::run_preferred_versions`]. The peer-hoist
    /// pickers look up only their missing-peer names, so this
    /// materializes a handful of buckets instead of a per-importer copy
    /// of the whole run history.
    pub(crate) fn preferred_versions_for_names<'name>(
        &self,
        seed: &pnpm_resolving_resolver_base::PreferredVersions,
        names: impl Iterator<Item = &'name str>,
    ) -> pnpm_resolving_resolver_base::PreferredVersions {
        let run = self.workspace.run_preferred_versions();
        let mut out = pnpm_resolving_resolver_base::PreferredVersions::new();
        for name in names {
            let mut bucket = seed.get(name).cloned().unwrap_or_default();
            if let Some(run_bucket) = run.versions.get(name) {
                for (selector, entry) in run_bucket {
                    bucket.entry(selector.clone()).or_insert_with(|| entry.clone());
                }
            }
            if !bucket.is_empty() {
                out.insert(name.to_string(), bucket);
            }
        }
        out
    }
}
