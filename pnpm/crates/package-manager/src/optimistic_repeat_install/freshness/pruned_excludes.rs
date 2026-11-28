use super::{
    OptimisticRepeatInstallCheck,
    Scope,
    WorkspaceState,
    drift,
};

pub(super) fn installed_versions(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    scope: Scope<'_>,
) -> Option<pnpm_config::version_policy::ResolvedPackageVersions> {
    let validation = state.validation.as_ref()?;
    let digest = validation.lockfile.as_deref()?;
    crate::catalog_cleanup::installed_prune_versions(
        check.config,
        check.workspace_root,
        digest,
        |lockfile| {
            if validation.lockfiles.is_empty() {
                return true;
            }
            let Scope::Selected(selected) = scope else { return false };
            drift::reached_lockfile_importers(lockfile, check.workspace_root, selected)
                .iter()
                .all(|id| {
                    validation
                        .project(id)
                        .is_some_and(|record| record.lockfile == Some(digest))
                })
        },
    )
}

pub(super) fn changed(check: &OptimisticRepeatInstallCheck<'_>, state: &WorkspaceState) -> bool {
    state.settings.minimum_release_age_exclude != check.config.minimum_release_age_exclude
        || state.settings.trust_policy_exclude != check.config.trust_policy_exclude
}
