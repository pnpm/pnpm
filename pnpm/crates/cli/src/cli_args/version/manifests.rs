use super::{Path, PathBuf, VersionArgs};
use miette::Context;
use pnpm_package_manifest::PackageManifest;
use pnpm_versioning::jsr_manifest_updates;

impl VersionArgs {
    /// Save the bumped `manifest` and set `new_version` in the JSR manifests
    /// next to it, writing nothing on a dry run. Returns the JSR manifests.
    pub(super) fn save_bumped_manifests(
        &self,
        manifest: &mut PackageManifest,
        pkg_dir: &Path,
        new_version: &str,
    ) -> miette::Result<Vec<PathBuf>> {
        let jsr_updates = jsr_manifest_updates(pkg_dir, new_version)?;
        if !self.dry_run {
            manifest
                .save()
                .wrap_err_with(|| format!("saving {}", manifest.path().display()))?;
            for update in &jsr_updates {
                update.write()?;
            }
        }
        Ok(jsr_updates
            .into_iter()
            .map(|update| update.path)
            .collect())
    }
}
