use super::{
    super::{effective_node_version, included_dependencies},
    InstallError, InstallExecution, InstallOwned, InstallRunOptions, InstallView,
};
use crate::install::state_options::ResolveOnly;
use pnpm_config::Config;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_store_dir::VerifiedFileIntegrity;
use std::{io::IsTerminal, path::PathBuf};

/// What the run's flags settle into before anything is read from disk.
pub(super) struct RunMode {
    pub(super) lockfile_only: bool,
    pub(super) resolve_only: bool,
    pub(super) prefer_frozen_lockfile: bool,
    pub(super) included: IncludedDependencies,
    pub(super) can_prompt: bool,
    pub(super) peer_issues_sink_is_none: bool,
    pub(super) effective_node_version: Option<String>,
    pub(super) verified_file_integrity_baseline: VerifiedFileIntegrity,
}

impl RunMode {
    pub(super) fn settle(
        install: InstallView<'_>,
        owned: &InstallOwned,
        options: &InstallRunOptions<'_, '_>,
    ) -> Result<Self, InstallError> {
        // Taken before any fetching so the store-verification figures
        // this install reports are its own — a recursive workspace run
        // and a long-lived embedder (the NAPI addon) both drive several
        // installs through the same process-global tally.
        let verified_file_integrity_baseline = VerifiedFileIntegrity::snapshot();
        reject_lockfile_only_without_lockfile(
            install.context.config,
            install.execution.lockfile_only,
        )?;
        let lockfile_only = effective_lockfile_only(
            install.context.config,
            install.execution.lockfile_only,
            options.rebuild.as_ref(),
        );
        reject_conflicting_store_config(install.context.config)?;
        Ok(Self {
            lockfile_only,
            // `--dry-run` resolves but never materializes, so it borrows the
            // lockfile-only plumbing (skip node_modules / `.modules.yaml` /
            // workspace-state) while additionally skipping the lockfile write.
            // Both lockfile-only paths must stop after writing the wanted lockfile:
            // neither may write `.modules.yaml`, the current lockfile, or workspace state.
            // The frozen path returns below; the fresh path returns in `complete_resolve_only`.
            resolve_only: lockfile_only || install.execution.dry_run,
            prefer_frozen_lockfile: !install.context.config.auto_dedupe
                && install.lockfile_policy.prefer_frozen.unwrap_or(
                    install.context.config.prefer_frozen_lockfile,
                ),
            // The same set the dependency-graph walker observes, written to
            // `.modules.yaml` as `included`.
            included: included_dependencies(&owned.projects.dependency_groups),
            can_prompt: options.prompt_eligibility_override.unwrap_or_else(prompts_are_answerable),
            peer_issues_sink_is_none: owned.resolution.peer_issues_sink.is_none(),
            effective_node_version: effective_node_version(
                install.context.config,
                install.context.manifest,
            ),
            verified_file_integrity_baseline,
        })
    }
}

impl RunMode {
    /// `enableModulesDir: false` without `--lockfile-only` or `--dry-run`:
    /// the run writes no `node_modules`, like a lockfile-only run, but still
    /// fetches the registry packages the host can install into the store, as
    /// pnpm's TypeScript engine
    /// did. Whoever mounts `node_modules` afterwards (a FUSE daemon, for
    /// one) serves it from the store instead of downloading each package on
    /// first access.
    pub(super) fn fetches_into_store(&self, execution: InstallExecution) -> bool {
        fetches_into_store(self.lockfile_only, execution)
    }

    /// Why the run materializes nothing, when it does not.
    pub(super) fn resolve_only(&self, execution: InstallExecution) -> Option<ResolveOnly> {
        if !self.resolve_only {
            None
        } else if self.fetches_into_store(execution) {
            Some(ResolveOnly::FetchIntoStore)
        } else {
            Some(ResolveOnly::LockfileOnly)
        }
    }
}

/// Whether a run whose `lockfile_only` is [`effective_lockfile_only`] fetches
/// into the store: only when neither `--lockfile-only` nor `--dry-run` asked
/// for the lockfile alone, which leaves `enableModulesDir: false`.
fn fetches_into_store(lockfile_only: bool, execution: InstallExecution) -> bool {
    lockfile_only && !execution.lockfile_only && !execution.dry_run
}

/// A prompt only reaches a person on an interactive terminal outside CI.
fn prompts_are_answerable() -> bool {
    !pnpm_config::is_ci() && std::io::stdin().is_terminal()
}

/// `--lockfile-only` with `lockfile: false` asks for a lockfile the run is
/// forbidden to write.
fn reject_lockfile_only_without_lockfile(
    config: &Config,
    lockfile_only: bool,
) -> Result<(), InstallError> {
    if lockfile_only && !config.lockfile {
        return Err(InstallError::ConfigConflictLockfileOnlyWithNoLockfile);
    }
    Ok(())
}

/// `enableModulesDir: false` (with the global virtual store off) is "resolve
/// and write the lockfile, materialize nothing" — the same pipeline
/// `--lockfile-only` takes, entered from config, except that the packages
/// are still fetched into the store (`RunMode::fetches_into_store`). It stays
/// outside the `lockfile: false` conflict (pnpm accepts that combination and
/// writes nothing), and never turns a rebuild — which runs against an
/// already-materialized `node_modules` — into a silent no-op.
fn effective_lockfile_only(
    config: &Config,
    lockfile_only: bool,
    rebuild: Option<&crate::RebuildOptions>,
) -> bool {
    lockfile_only
        || (rebuild.is_none() && !config.enable_modules_dir && !config.enable_global_virtual_store)
}

fn reject_conflicting_store_config(config: &Config) -> Result<(), InstallError> {
    if config.frozen_store && config.force {
        return Err(InstallError::ConfigConflictFrozenStoreWithForce);
    }
    if config.virtual_store_only
        && !config.enable_modules_dir
        && !config.enable_global_virtual_store
    {
        return Err(InstallError::ConfigConflictVirtualStoreOnlyWithNoModulesDir);
    }
    Ok(())
}

/// Restores the workspace manifest files a catalog prune rewrote when the
/// install that pruned them does not finish.
pub(super) struct WorkspaceManifestRollbackGuard {
    files: Vec<FileRollback>,
    committed: bool,
}

/// One file the prune rewrote: what it held before, and what the prune left.
struct FileRollback {
    path: PathBuf,
    original_content: String,
    pruned_content: Option<String>,
}

impl WorkspaceManifestRollbackGuard {
    /// The current content of the workspace manifest of `workspace_dir` and
    /// the local files it includes, to pass to [`Self::new`] once the prune
    /// has run. A symlinked include is recorded by its target, which is the
    /// file the prune writes. An include index that does not load names only the root
    /// manifest, and the prune reports it.
    pub(super) fn snapshot(workspace_dir: &std::path::Path) -> Vec<(PathBuf, Option<String>)> {
        let root = workspace_dir.join(pnpm_workspace::WORKSPACE_MANIFEST_FILENAME);
        let included = pnpm_workspace::read_workspace_manifest_document(workspace_dir)
            .ok()
            .flatten()
            .and_then(|document| document.includes)
            .map(|includes| {
                includes
                    .local_paths()
                    .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        std::iter::once(root)
            .chain(included)
            .map(|path| {
                let content = std::fs::read_to_string(&path).ok();
                (path, content)
            })
            .collect()
    }

    /// A guard over the files of `snapshot` the prune changed, if any.
    pub(super) fn new(snapshot: Vec<(PathBuf, Option<String>)>) -> Option<Self> {
        let files: Vec<FileRollback> = snapshot
            .into_iter()
            .filter_map(|(path, original)| FileRollback::new(path, original?))
            .collect();
        (!files.is_empty()).then_some(Self { files, committed: false })
    }

    pub(super) fn commit(mut self) {
        self.committed = true;
    }
}

impl FileRollback {
    fn new(path: PathBuf, original_content: String) -> Option<Self> {
        let pruned_content = read_if_present(&path).ok()?;
        (Some(&original_content) != pruned_content.as_ref()).then_some(Self {
            path,
            original_content,
            pruned_content,
        })
    }

    /// Put the original content back, unless something other than the prune
    /// has changed the file since.
    fn restore(&self) {
        if read_if_present(&self.path).is_ok_and(|current| current == self.pruned_content) {
            let _ = pnpm_fs::write_atomic(&self.path, self.original_content.as_bytes());
        }
    }
}

fn read_if_present(path: &std::path::Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

impl Drop for WorkspaceManifestRollbackGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.files.iter().for_each(FileRollback::restore);
        }
    }
}

#[cfg(test)]
mod tests;
