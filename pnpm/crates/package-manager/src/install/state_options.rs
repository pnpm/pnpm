use super::{
    Arc, Catalogs, Config, HashSet, IncludedDependencies, Lockfile, Modules, NodeLinker,
    PackageManifest, Path, PathBuf, ProjectMutation, RebuildOptions, ResolutionVerifier,
    WorkspaceInstallSelection, WorkspaceState,
};
use pnpm_store_dir::VerifiedFileIntegrity;

#[derive(Clone, Copy)]
pub(crate) struct CompletionMode {
    pub(crate) resolve_only: bool,
    pub(crate) dry_run: bool,
    pub(crate) peer_issues_sink_is_none: bool,
}

pub(crate) struct ApplyPriorState {
    pub(crate) lockfile: Option<Lockfile>,
    pub(crate) layout: Option<pnpm_modules_yaml::ModulesLayout>,
    pub(crate) metadata: Option<Modules>,
    pub(crate) is_inconsistent: bool,
    /// See [`RecordedWorkspace::moved`].
    pub(crate) tree_moved: bool,
}

pub(crate) struct ApplyProjectSelection<'a> {
    pub(crate) importers: SelectedImporters<'a>,
    pub(crate) workspace_packages: Option<pnpm_resolving_resolver_base::WorkspacePackages>,
    pub(crate) workspace_root: PathBuf,
    pub(crate) included: IncludedDependencies,
    pub(crate) node_linker: NodeLinker,
    pub(crate) filtered_install: bool,
    pub(crate) supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
}

pub(crate) struct PendingProjectScripts<'a, 'selection> {
    pub(crate) mutation: ProjectMutation,
    pub(crate) manifest_dir: &'a Path,
    pub(crate) selection: Option<WorkspaceInstallSelection<'selection>>,
    pub(crate) rebuild: Option<RebuildOptions>,
}

pub(crate) struct ApplyCompletionContext {
    pub(crate) prefix: String,
    pub(crate) workspace_manifest_dir: PathBuf,
    pub(crate) catalogs: Catalogs,
    pub(crate) catalog_context_present: bool,
    pub(crate) verified_file_integrity_baseline: VerifiedFileIntegrity,
    pub(crate) config: &'static Config,
}

#[derive(Clone, Copy)]
pub(crate) struct LockfileWritePolicy {
    pub(crate) synthesized_from_current: bool,
    pub(crate) fast_updated: bool,
    pub(crate) save: bool,
}

pub(crate) struct ApplyResolutionState<'a> {
    pub(crate) existing_wanted: Option<&'a Lockfile>,
    pub(crate) loaded: Option<&'a Lockfile>,
    pub(crate) frozen: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ModulesTreeContext<'a> {
    pub(crate) config: &'static Config,
    pub(crate) workspace_root: &'a Path,
    pub(crate) node_linker: NodeLinker,
    pub(crate) included: IncludedDependencies,
}

#[derive(Clone, Copy)]
pub(crate) struct PriorModulesState<'a> {
    pub(crate) layout: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(crate) metadata: Option<&'a Modules>,
    pub(crate) is_inconsistent: bool,
    pub(crate) filtered_install: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CommittedLockfiles<'a> {
    pub(crate) materialized: Option<&'a Lockfile>,
    pub(crate) selected: Option<&'a Lockfile>,
    pub(crate) wanted: Option<&'a Lockfile>,
}

pub(crate) struct CommittedBuildState<'a> {
    pub(crate) ignored_builds: &'a [String],
    pub(crate) deferred_builds: Vec<String>,
    pub(crate) rebuild: Option<&'a RebuildOptions>,
}

#[derive(Clone, Copy)]
pub(crate) struct CommittedProjects<'a> {
    pub(crate) manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(crate) skipped: &'a crate::SkippedSnapshots,
    pub(crate) frozen: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct PeerIssueLockfiles<'a> {
    pub(crate) wanted: Option<&'a Lockfile>,
    pub(crate) fresh: Option<&'a Lockfile>,
    pub(crate) importer_ids: &'a HashSet<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct ProjectScriptSelection<'a, 'selection> {
    pub(crate) mutation: ProjectMutation,
    pub(crate) manifest_dir: &'a Path,
    pub(crate) workspace: Option<&'a WorkspaceInstallSelection<'selection>>,
    pub(crate) rebuild: Option<&'a RebuildOptions>,
}

#[derive(Clone, Copy)]
pub(crate) struct CompletionWorkspace<'a> {
    pub(crate) config: &'static Config,
    pub(crate) catalogs: Option<&'a Catalogs>,
    pub(crate) workspace_root: &'a Path,
    pub(crate) workspace_manifest_dir: &'a Path,
}

#[derive(Clone, Copy)]
pub(crate) struct SelectedLockfiles<'a> {
    pub(crate) fresh: Option<&'a Lockfile>,
    pub(crate) wanted: Option<&'a Lockfile>,
    pub(crate) current: Option<&'a Lockfile>,
}

#[derive(Clone, Copy)]
pub(crate) struct SelectedImporters<'a> {
    pub(crate) requested_ids: Option<&'a HashSet<String>>,
    pub(crate) real_ids: &'a HashSet<String>,
    pub(crate) manifests: &'a [(PathBuf, &'a PackageManifest)],
}

pub(crate) struct LockfileVerificationInputs<'a, 'install> {
    pub(crate) verifiers: &'a [Arc<dyn ResolutionVerifier>],
    pub(crate) path: Option<&'a Path>,
    pub(crate) override_check: Option<super::LockfileVerificationOverride<'install>>,
}

#[derive(Clone, Copy)]
pub(crate) struct InstallProjectMetadata<'a> {
    pub(crate) catalogs: &'a Catalogs,
    pub(crate) manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(crate) prefix: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct RepeatInstallPolicy<'a> {
    pub(crate) frozen: bool,
    pub(crate) filtered: bool,
    pub(crate) disable_optimistic_check: bool,
    pub(crate) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(crate) rebuild: Option<&'a RebuildOptions>,
    pub(crate) effective_node_version: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub(crate) struct PreparedLockfiles<'a> {
    pub(crate) wanted: Option<&'a Lockfile>,
    pub(crate) current: Option<&'a Lockfile>,
    pub(crate) importer_ids: Option<&'a HashSet<String>>,
}

#[derive(Clone, Copy)]
pub(crate) struct PruneEligibility {
    pub(crate) resolve_only: bool,
    pub(crate) is_inconsistent: bool,
    pub(crate) filtered_install: bool,
}

/// What the last install recorded about the workspace, against the projects
/// the tree holds now.
#[derive(Clone, Copy)]
pub(crate) struct RecordedWorkspace<'a> {
    pub(crate) state: Option<&'a WorkspaceState>,
    /// A known or potentially moved tree, where moves are supported
    /// ([`crate::install::tree_may_move`]). No current project is one `state`
    /// records, or an existing tree has no readable state to prove its origin.
    /// Its bins may still name where it was.
    pub(crate) moved: bool,
    pub(crate) projects: &'a [(PathBuf, &'a PackageManifest)],
}
