use super::{
    InstallError,
    InstallScope,
    RunExecution,
};
use pnpm_package_manifest::PackageManifest;
use std::path::PathBuf;

impl RunExecution<'_> {
    /// Whether a malformed dependency declaration has to keep the repeat-install
    /// shortcut from firing. The repeat-install path bypasses readPackage hooks
    /// and the manifest reader skips non-string specifiers, so a verdict reached
    /// from such a manifest was computed from an incomplete dependency list. A
    /// declaration that needs fixing must reach those hooks, which may repair it,
    /// before the install can decide that the existing lockfile is current.
    pub(super) fn malformed_declaration_blocks_repeat_install(
        &self,
        scope: &InstallScope<'_>,
    ) -> bool {
        self.install.execution.mutation.uses_project_manifests()
            && scope.project_manifests
                .iter()
                .any(|(_, manifest)| manifest.validate_dependency_types().is_err())
    }

    pub(super) fn validate_project_manifests(
        &self,
        manifests: &[(PathBuf, &PackageManifest)],
    ) -> Result<(), InstallError> {
        if !self.install.execution.mutation.uses_project_manifests() {
            return Ok(());
        }
        for (_, manifest) in manifests {
            manifest.validate_dependency_types().map_err(InstallError::InvalidProjectManifest)?;
        }
        Ok(())
    }
}
