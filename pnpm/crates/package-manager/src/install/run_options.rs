use std::{
    collections::HashSet,
    path::{
        Path,
        PathBuf,
    },
};

use pnpm_package_manifest::PackageManifest;

use super::{
    LockfileCheckSink,
    PreparedFrozenRun,
    WorkspaceInstallSelection,
};
use crate::{
    LockfileVerificationOverride,
    RebuildOptions,
};

pub(crate) struct InstallRunOptions<'install, 'selection> {
    /// The repeat-install fast path the run consults before it sets up
    /// anything.
    pub(crate) repeat_install: crate::install_state::RepeatInstallCheck,
    pub(crate) verification: InstallVerificationOptions<'install>,
    pub(crate) rebuild: Option<RebuildOptions>,
    pub(crate) selection: Option<WorkspaceInstallSelection<'selection>>,
    pub(crate) root_manifest_as_workspace_root: bool,
    pub(crate) save: InstallSaveOptions,
    /// Forces the interactive-prompt eligibility that is otherwise derived
    /// from the process environment, so tests can exercise both branches.
    pub(crate) prompt_eligibility_override: Option<bool>,
    pub(crate) manifests: InstallManifestOptions<'install>,
}

/// Per-run lockfile verification overrides and workspace scope extensions.
#[derive(Default)]
pub(crate) struct InstallVerificationOptions<'install> {
    pub(crate) override_check: Option<LockfileVerificationOverride<'install>>,
    /// Policy snapshot recorded during `prepare_frozen`; when `Some`, `override_check`
    /// is reused only if every active verifier trusts this snapshot.
    pub(crate) prepared_policy: Option<serde_json::Map<String, serde_json::Value>>,
    /// Additional workspace-project directories relative to the lockfile dir
    /// (used by `pnpm deploy` for sibling workspace projects rewritten to `file:`).
    pub(crate) extra_workspace_dirs: Vec<String>,
}

/// The workspace files the run may write besides the installed modules.
pub(crate) struct InstallSaveOptions {
    /// pnpm's `saveLockfile`: whether the resolved graph may be written
    /// to `<workspace_root>/pnpm-lock.yaml`. `false` for an install
    /// whose resolution belongs to a project other than the one that
    /// owns that lockfile, so the run must leave it untouched.
    pub(crate) lockfile: bool,
    /// Whether the run may record itself in
    /// `node_modules/.pnpm-workspace-state-v1.json`. `false` for an install
    /// whose importers are not the workspace's projects, so that state
    /// keeps describing the workspace's last install.
    pub(crate) workspace_state: bool,
    pub(crate) pending_state: Option<super::PendingWorkspaceState>,
    /// pnpm's `lockfileCheck`: the run hands the wanted lockfile to the sink
    /// instead of writing it, and must leave nothing else on disk changed
    /// either. Only `pacquet dedupe --check` sets it.
    pub(crate) lockfile_check: Option<LockfileCheckSink>,
    /// Overrides `lockfile_policy.trust` when recording `trustLockfile` in
    /// `.pnpm-workspace-state-v1.json`, for `pnpr` link passes that skip local
    /// verification after verifying against `pnpr` under the user's policy.
    pub(crate) recorded_trust_lockfile: Option<bool>,
}

impl Default for InstallSaveOptions {
    fn default() -> Self {
        InstallSaveOptions {
            lockfile: true,
            workspace_state: true,
            pending_state: None,
            lockfile_check: None,
            recorded_trust_lockfile: None,
        }
    }
}

impl InstallSaveOptions {
    pub(crate) fn effective_trust_lockfile(&self, trust_lockfile: bool) -> bool {
        self.recorded_trust_lockfile.unwrap_or(trust_lockfile)
    }

    /// Whether the run writes the wanted lockfile to disk.
    pub(crate) fn writes_lockfile(&self) -> bool {
        self.lockfile && self.lockfile_check.is_none()
    }
}

#[derive(Default)]
pub(crate) struct InstallManifestOptions<'install> {
    pub(crate) deploy_hook: bool,
    pub(crate) overrides_root_dir: Option<&'install Path>,
    /// pnpm's `pruneDirectDependencies`: scan each importer's `node_modules`
    /// and remove extraneous direct entries not recorded in the wanted
    /// lockfile or public hoist set. Only `pacquet prune` sets it.
    pub(crate) prune_direct_dependencies: bool,
    /// Project manifests used only as the source for lockfile importer
    /// specifiers. `pacquet update --no-save` resolves against an in-memory
    /// manifest rewrite but must serialize importer specifiers from the
    /// manifest the user kept on disk. Supplied already
    /// `readPackage`-transformed.
    pub(crate) specifier_manifests: Option<Vec<(PathBuf, PackageManifest)>>,
    /// Manifest paths `pacquet update --no-save` already ran `readPackage`
    /// over before preparing its in-memory resolution rewrite. The hook must
    /// observe each project manifest exactly once, so the install layer skips
    /// these and still hooks every project manifest outside the set, the
    /// workspace projects the non-selected update path never loads. Dependency
    /// manifests always flow through the resolver's hook path.
    pub(crate) hooked_paths: HashSet<PathBuf>,
    /// See [`crate::ManifestSpecBumps`]. Only `pacquet update` sets it.
    pub(crate) spec_bumps: Option<&'install crate::ManifestSpecBumps>,
    /// Pre-seeded `requiresBuild` verdicts from a `pnpr` resolve stream.
    pub(crate) known_requires_build: Option<pnpm_deps_restorer::RequiresBuildBySnapshot>,
    /// Hooked manifests and `preResolution` output prepared during `pnpm ci`
    /// frozen validation, so the post-clean install does not re-run hooks.
    pub(crate) prepared_frozen: Option<PreparedFrozenRun>,
}
