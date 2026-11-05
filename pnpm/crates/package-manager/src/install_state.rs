//! What a completed install records in the workspace state, and the inputs
//! a later command compares with those records. The install writes the
//! records and the repeat-install fast path reads them. Both depend on this
//! module, so the install never depends on the fast path.

pub(crate) mod local_file_deps;
pub(crate) mod settings;
pub(crate) mod timestamps;
pub(crate) mod validation;
pub(crate) use local_file_deps::{
    FrozenLocalTarballCheck, frozen_local_tarballs_to_verify, has_local_file_dep_requiring_install,
    has_local_file_override, has_local_file_package_extension,
};
pub use manifest_source::{DeclaredManifests, ManifestHooks, RepeatInstallManifests};
pub(crate) use settings::{
    ResolvedImporters, catalogs_cache_matches, current_settings_with_catalogs, first_setting_drift,
    linked_platform_missing, linked_platform_to_record, recorded_default_registry,
    recorded_extend_node_path_matches, recorded_platforms_match, recorded_registry_prefixes,
    recorded_registry_routes_after_install, records_registry_keys_of,
};
pub(crate) use timestamps::{
    file_mtime, file_mtime_from_metadata, filesystem_now_ms_in_modules,
    filesystem_now_ns_in_modules, last_validated_timestamp, lockfile_modified_since,
    modified_at_or_after,
};

mod manifest_source;

use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::WorkspaceState;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// The repeat-install check an install consults before it sets up anything,
/// for the projects `Scope` covers. The fast path supplies it, so that the
/// install does not depend on the fast path.
pub(crate) type RepeatInstallCheck = fn(&OptimisticRepeatInstallCheck<'_>, Scope<'_>) -> Decision;

/// Project manifests keyed by their lexically normalized project directory.
pub(crate) type ProjectManifestsByDir<'a> = HashMap<PathBuf, &'a PackageManifest>;

/// Outcome of [`crate::check_optimistic_repeat_install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The install is fully up to date — emit "Already up to date"
    /// and exit before any of the install setup runs.
    UpToDate,
    /// Fall through to the full install path. `reason` is a short
    /// diagnostic string surfaced via `tracing::debug!` for
    /// diagnosability without contaminating the reporter stream.
    Skipped { reason: &'static str },
}

/// Inputs to [`crate::check_optimistic_repeat_install`].
pub struct OptimisticRepeatInstallCheck<'a> {
    /// The root the install recorded its lockfile and workspace state
    /// against, which importer ids and relative local `pnpm.overrides`
    /// targets are named from too. The directory containing
    /// `pnpm-workspace.yaml` (or the project root when no workspace
    /// manifest exists — same fallback as
    /// [`Install::run`](crate::Install::run)), unless the configuration
    /// pins the lockfile somewhere else.
    pub workspace_root: &'a Path,
    pub config: &'a Config,
    /// Every importer's `(root_dir, manifest)` pair. For a
    /// single-project install it's just the root manifest; for a
    /// workspace install it's every project the resolver would
    /// otherwise walk. The caller passes this in (rather than this
    /// function rediscovering it) so the same walk seeds the regular
    /// install path on the fall-through.
    pub project_manifests: DeclaredManifests<'a>,
    /// `true` when a `pnpm-workspace.yaml` drives the install — that
    /// selects the workspace branch, which keys the manifest and
    /// lockfile comparisons off `lastValidatedTimestamp`. `false` (no
    /// workspace manifest) selects the single-project branch, which
    /// additionally requires `pnpm-lock.yaml` to exist on disk —
    /// `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` is raised otherwise, which
    /// resolves to not-up-to-date — and keys its comparisons off the
    /// lockfile mtimes instead.
    pub is_workspace_install: bool,
    /// The wanted lockfile (`None` once loaded when `pnpm-lock.yaml`
    /// is absent or empty). Consulted only by the modified-manifests
    /// content re-check; the pure-mtime fast path never parses it —
    /// which is why it arrives lazily, so the common repeat-install
    /// run skips the YAML parse entirely. A separately bounded byte
    /// scan only runs when lockfile metadata changed. When absent and
    /// `<install_state_dir>/lock.yaml` exists, the current lockfile
    /// stands in as the wanted one — it records exactly what the
    /// previous install materialized — and `pnpm-lock.yaml` is
    /// regenerated from it before the check reports up-to-date.
    pub lockfile: MaybeLazyLockfile<'a>,
    /// Catalogs from the workspace manifest or an `updateConfig`
    /// pnpmfile hook, for resolving `catalog:` values inside
    /// `pnpm.overrides` before the lockfile settings comparison.
    pub catalogs: &'a Catalogs,
    pub layout: RepeatInstallLayout<'a>,
    pub manifests: RepeatInstallManifests<'a>,
}

impl<'a> OptimisticRepeatInstallCheck<'a> {
    /// The projects whose commands the importers link, per
    /// [`RepeatInstallManifests::workspace_projects`].
    pub(crate) fn bin_projects(&self) -> &'a [(PathBuf, &'a PackageManifest)] {
        self.manifests.workspace_projects.unwrap_or(self.project_manifests.as_slice())
    }
}

#[derive(Clone, Copy)]
pub struct RepeatInstallLayout<'a> {
    pub node_linker: NodeLinker,
    pub included: IncludedDependencies,
    /// The CLI-merged effective `supportedArchitectures` this run would
    /// install with (yaml plus `--cpu` / `--os` / `--libc`), compared
    /// against the recorded value like `included`.
    pub supported_architectures: Option<&'a SupportedArchitectures>,
}

/// The projects a gate holds to being installed. Whatever the gate reports
/// must be something the install it leads to can fix: a filtered install
/// leaves every project outside its selection as it is.
#[derive(Clone, Copy)]
pub(crate) enum Scope<'a> {
    /// Every project, for an unfiltered install.
    Workspace,
    /// The selected projects and the workspace projects they depend on, for a
    /// filtered install or a filtered command.
    Selected(&'a [&'a Path]),
    /// An unfiltered command run from the selected projects. Its install
    /// covers the whole workspace, so a project whose recorded install went
    /// stale is held anywhere. A project never installed is held only where
    /// the selection depends on it, so an install filtered to other projects
    /// does not make every command install again.
    Command(&'a [&'a Path]),
}

pub(crate) fn manifest_has_runtime_deps(manifest: &PackageManifest) -> bool {
    let value = manifest.value();
    [value.get("dependencies"), value.get("devDependencies"), value.get("optionalDependencies")]
        .into_iter()
        .flatten()
        .any(|deps| {
            deps.as_object()
                .is_some_and(|map| !map.is_empty())
        })
}

/// Whether the manifest declares at least one peer that is not marked
/// optional. The caller decides whether `autoInstallPeers` fetches them.
pub(crate) fn manifest_has_required_peers(manifest: &PackageManifest) -> bool {
    let Some(peers) = manifest
        .value()
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    let meta = manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object);
    peers
        .keys()
        .any(|name| !peer_dependency_is_optional(meta, name))
}

fn peer_dependency_is_optional(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
    name: &str,
) -> bool {
    meta.and_then(|meta| meta.get(name))
        .and_then(serde_json::Value::as_object)
        .and_then(|entry| entry.get("optional"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

/// The pnpmfile list recorded in the workspace state and compared by
/// the freshness check: every pnpmfile the install loads. An install
/// that ignores the pnpmfile records none, so the next install that
/// honors it again sees the list change and re-validates.
pub(crate) fn current_pnpmfiles(workspace_root: &Path, config: &Config) -> Vec<String> {
    if config.ignore_pnpmfile {
        return Vec::new();
    }
    crate::find_install_pnpmfiles(config, workspace_root)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Whether no current project dir is a key of `state`.
pub(crate) fn recorded_elsewhere(
    state: &WorkspaceState,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    !project_manifests
        .iter()
        .any(|(root_dir, _)| state.projects.contains_key(root_dir.to_string_lossy().as_ref()))
}

/// Whether `current` already records what materializing `wanted` would
/// produce.
///
/// The current lockfile keeps only what the importers reach
/// ([`crate::filter_lockfile_for_current`]) and none of the top-level keys
/// pnpm does not define, so a wanted lockfile carrying a snapshot no
/// importer reaches any more, or an embedder's extension block, can never
/// equal it. Comparing the same shape both sides is what lets such a tree
/// settle instead of re-materializing on every run.
///
/// The equal case is the common one and answers without building the
/// filtered shape at all. Under `config.skip_runtimes` the direct runtimes are
/// left out of the shape, so a tree that still holds one does not match.
pub(crate) fn materialized_shape_matches(
    wanted: &Lockfile,
    current: &Lockfile,
    (included, supported): (
        IncludedDependencies,
        Option<&pnpm_package_is_installable::SupportedArchitectures>,
    ),
    config: &Config,
) -> bool {
    let mut skipped = crate::SkippedSnapshots::new();
    if config.skip_runtimes
        && let Some(packages) = wanted.packages.as_ref()
    {
        crate::add_direct_runtime_skips(&mut skipped, &wanted.importers, packages);
    }
    if skipped.is_empty() && wanted == current {
        return true;
    }
    // A transient skip (a failed optional fetch) prunes the current lockfile
    // further, and its set is not known here. Such a tree falls
    // through to materialization, which retries the fetch anyway.
    current
        == &crate::filter_lockfile_for_current(
            wanted,
            &crate::GroupSelection::classify(wanted, included, config.peer_edge_options())
                .linking_for(supported),
            &skipped,
        )
}

/// Where the hoisted linker nests the dependencies of the workspace project at
/// `root_dir` that it cannot hoist to the root. The walker always uses
/// `node_modules` here, whatever `modulesDir` says.
pub(crate) fn hoisted_project_modules_dir(root_dir: &Path) -> PathBuf {
    root_dir.join("node_modules")
}
