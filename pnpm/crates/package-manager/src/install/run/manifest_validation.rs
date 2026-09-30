use super::{InstallError, InstallScope, Reporter, RunExecution};
use crate::ProjectMutation;
use pnpm_package_manifest::PackageManifest;
use std::path::PathBuf;

impl RunExecution<'_> {
    pub(super) fn can_skip_install<Report: Reporter>(
        &self,
        scope: &InstallScope<'_>,
    ) -> Result<bool, InstallError> {
        // The repeat-install path bypasses readPackage hooks. A malformed
        // declaration must reach those hooks, which may repair it, before the
        // install can decide that the existing lockfile is current.
        if self.install.execution.mutation != ProjectMutation::NoInstall
            && scope.project_manifests
                .iter()
                .any(|(_, manifest)| manifest.validate_dependency_types().is_err())
        {
            return Ok(false);
        }
        scope.is_already_up_to_date::<Report>(
            self.install,
            &self.owned,
            &self.mode,
            &self.workspace,
        )
    }

    pub(super) fn validate_project_manifests(
        &self,
        manifests: &[(PathBuf, &PackageManifest)],
    ) -> Result<(), InstallError> {
        if self.install.execution.mutation == ProjectMutation::NoInstall {
            return Ok(());
        }
        for (_, manifest) in manifests {
            manifest.validate_dependency_types().map_err(InstallError::InvalidProjectManifest)?;
        }
        Ok(())
    }
}
