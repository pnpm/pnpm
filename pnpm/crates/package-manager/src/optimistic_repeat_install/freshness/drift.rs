//! Which projects the last validation still proves installed, and which of
//! the others a gate holds.

use super::{
    super::{
        ManifestStat, OptimisticRepeatInstallCheck, Path, PathBuf, WorkspaceState,
        current_lockfile::CurrentLockfile,
        validation::{FileDigest, FileDigests},
    },
    Scope,
};
use pnpm_workspace::importer_id_from_root_dir;
use pnpm_workspace_state::{FileSignature, Validation};
use std::collections::HashSet;

/// Whether a project is still proven installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectStatus {
    Proven,
    /// The manifest is not the one the project was proven installed with.
    ManifestChanged,
    /// The lockfile is not the one the project was proven installed with, and
    /// the manifest may not be either.
    LockfileChanged,
    /// No validation records the project as installed.
    Unrecorded,
}

impl ProjectStatus {
    /// Whether the current lockfile has to show the project installed: its
    /// install was never proven against this lockfile.
    pub(crate) fn needs_install_proof(self) -> bool {
        matches!(self, Self::LockfileChanged | Self::Unrecorded)
    }
}

/// Every input the content phase compares against the last validation.
pub(crate) struct InputDrift<'a> {
    pub(crate) stats: Vec<ManifestStat<'a>>,
    /// Every phase of the check reads the current lockfile from here.
    pub(super) current: CurrentLockfile<'a>,
    lockfile_path: PathBuf,
    /// The lockfile the project records are compared with, as it was when
    /// [`Self::compute`] read it.
    pub(super) lockfile: FileDigest,
    /// Whether the lockfile's content differs from what the last validation
    /// recorded.
    pub(super) lockfile_changed: bool,
    /// Whether a configured patch file's content differs from what the last
    /// validation proved.
    pub(super) patches_modified: bool,
    /// Whether an input's stat no longer stood for its content and the
    /// content was read again.
    pub(super) rehashed: bool,
}

impl<'a> InputDrift<'a> {
    pub(crate) fn compute(
        check: &OptimisticRepeatInstallCheck<'a>,
        state: &WorkspaceState,
    ) -> Self {
        let recorded = state.validation.as_ref();
        let digests = FileDigests::new(recorded);
        let (lockfile_path, lockfile) = digests.lockfile(check.workspace_root, check.config);
        let recorded_lockfile = recorded_signature(recorded, &lockfile_path);
        let lockfile_rehashed = matches!(
            (&lockfile, recorded_lockfile),
            (FileDigest::Present(now), Some(then)) if now.digest == then.digest,
        ) && !digests.trusts(&lockfile_path);
        let lockfile_changed = recorded.is_some()
            && lockfile.digest()
                != recorded_lockfile.map(|signature| signature.digest.as_str());
        let patches = patch_drift(check, state, &digests);
        let stats = project_stats(check, recorded, lockfile.digest());
        Self {
            stats,
            current: CurrentLockfile::new(&check.config.install_state_dir),
            lockfile_path,
            lockfile,
            lockfile_changed,
            patches_modified: patches.modified,
            rehashed: lockfile_rehashed || patches.rehashed,
        }
    }

    pub(crate) fn all_proven(&self) -> bool {
        self.stats
            .iter()
            .all(|stat| stat.status == ProjectStatus::Proven)
    }

    /// The projects `scope` holds that are not proven installed.
    pub(super) fn held(
        &self,
        check: &OptimisticRepeatInstallCheck<'_>,
        scope: Scope<'_>,
    ) -> Vec<&ManifestStat<'a>> {
        let unproven = self.stats
            .iter()
            .filter(|stat| stat.status != ProjectStatus::Proven);
        let (selected, held_anywhere): (&[&Path], fn(ProjectStatus) -> bool) = match scope {
            Scope::Workspace => return unproven.collect(),
            Scope::Selected(selected) => (selected, |_| false),
            Scope::Command(selected) => (selected, |status| status != ProjectStatus::Unrecorded),
        };
        let mut reached = None;
        unproven
            .filter(|stat| {
                held_anywhere(stat.status)
                    || reached
                        .get_or_insert_with(|| reached_importers(check, &self.current, selected))
                        .as_ref()
                        .is_none_or(|reached| {
                            reached.contains(&importer_id_from_root_dir(
                                check.workspace_root,
                                stat.root_dir,
                            ))
                        })
            })
            .collect()
    }

    /// The projects a passing check proves installed: those already proven
    /// and the `held` ones it checked.
    pub(super) fn proven_dirs(&self, held: &[&ManifestStat<'_>]) -> HashSet<PathBuf> {
        self.stats
            .iter()
            .filter(|stat| stat.status == ProjectStatus::Proven)
            .chain(held.iter().copied())
            .map(|stat| stat.root_dir.to_path_buf())
            .collect()
    }

    /// Whether the lockfile still has the stat [`Self::compute`] saw. A
    /// content check that passed validated the lockfile it read, so one
    /// rewritten since must not be recorded as validated.
    pub(crate) fn lockfile_unchanged(&self) -> bool {
        match (&self.lockfile, super::super::validation::FileStat::at(&self.lockfile_path)) {
            (FileDigest::Present(then), Some(now)) => now.matches(then),
            (FileDigest::Absent, None) => true,
            _ => false,
        }
    }
}

/// The signature `recorded` holds for the file at `path`.
fn recorded_signature<'v>(
    recorded: Option<&'v Validation>,
    path: &Path,
) -> Option<&'v FileSignature> {
    recorded?.files.get(&*path.to_string_lossy())
}

/// How the configured patch files compare with the last validation.
struct PatchDrift {
    /// Whether a patch's content differs from what the last validation proved.
    modified: bool,
    /// Whether a patch's stat no longer stood for its content.
    rehashed: bool,
}

fn patch_drift(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    digests: &FileDigests<'_>,
) -> PatchDrift {
    let mut drift = PatchDrift { modified: false, rehashed: false };
    for patch in
        super::super::validation::shared_input_paths(check.workspace_root, check.config, &[])
    {
        match (digests.of(&patch), recorded_signature(state.validation.as_ref(), &patch)) {
            (FileDigest::Present(now), Some(then)) => {
                drift.modified |= now.digest != then.digest;
                drift.rehashed |= !digests.trusts(&patch);
            }
            // A state without validation records has no digests, and its
            // timestamp is the only evidence there is. A patch added since changed
            // `patchedDependencies`, which the settings comparison catches.
            (FileDigest::Present(_), None) => {
                drift.modified |=
                    super::super::patch_modified_since(&patch, state.last_validated_timestamp);
            }
            (FileDigest::Absent, None) => {}
            _ => drift.modified = true,
        }
    }
    drift
}

/// Every project's [`ManifestStat`] against `recorded`, which proved the
/// projects against the lockfile whose digest was `lockfile_digest`.
fn project_stats<'a>(
    check: &OptimisticRepeatInstallCheck<'a>,
    recorded: Option<&Validation>,
    lockfile_digest: Option<&str>,
) -> Vec<ManifestStat<'a>> {
    super::super::manifest_agreement::manifest_stats(check.project_manifests, |root_dir, digest| {
        let importer_id = importer_id_from_root_dir(check.workspace_root, root_dir);
        let Some(record) = recorded.and_then(|validation| validation.project(&importer_id)) else {
            return ProjectStatus::Unrecorded;
        };
        // A changed lockfile calls for every check a changed manifest does,
        // and more.
        if record.lockfile != lockfile_digest {
            ProjectStatus::LockfileChanged
        } else if record.manifest != digest {
            ProjectStatus::ManifestChanged
        } else {
            ProjectStatus::Proven
        }
    })
}

/// The importer ids `selected` reaches through workspace links in the wanted
/// lockfile, or the current one standing in for it. `None` when neither can
/// be read, which leaves every project held.
fn reached_importers(
    check: &OptimisticRepeatInstallCheck<'_>,
    current: &CurrentLockfile<'_>,
    selected: &[&Path],
) -> Option<HashSet<String>> {
    let selected_ids: HashSet<String> = selected
        .iter()
        .map(|dir| importer_id_from_root_dir(check.workspace_root, dir))
        .collect();
    let lockfile = match check.lockfile.get() {
        Ok(Some(wanted)) => wanted,
        Ok(None) => current.get()??,
        Err(_) => return None,
    };
    let mut reached = crate::materialization_closure(
        lockfile,
        check.workspace_root,
        &selected_ids,
        &crate::GroupSelection::all(),
        &crate::SkippedSnapshots::new(),
    )
    .importer_ids;
    reached.extend(selected_ids);
    Some(reached)
}
