//! The project manifest the content check compares with the lockfile.

use super::{OptimisticRepeatInstallCheck, PackageManifest, Path};

/// The transforms the resolver applies to a project manifest without running
/// a hook: `packageExtensions`, then the overrides.
pub(super) struct ManifestTransforms<'a> {
    extender: Option<crate::PackageExtender>,
    pub(super) parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    overrider: Option<crate::VersionsOverrider>,
}

impl<'a> ManifestTransforms<'a> {
    pub(super) fn new(
        check: &OptimisticRepeatInstallCheck<'_>,
        parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    ) -> Result<Self, &'static str> {
        let extender = check.config.package_extensions
            .as_ref()
            .map(crate::PackageExtender::new)
            .transpose()
            .map_err(|_| "packageExtensions cannot be parsed")?;
        let overrider = parsed_overrides.map(|parsed| {
            crate::VersionsOverrider::new(parsed, check.workspace_root)
        });
        Ok(Self { extender, parsed_overrides, overrider })
    }

    /// `manifest` with the `packageExtensions` that match it.
    pub(super) fn extended<'m>(
        &self,
        manifest: &'m PackageManifest,
    ) -> std::borrow::Cow<'m, PackageManifest> {
        let Some(extender) = self.extender
            .as_ref()
            .filter(|extender| extender.matches(manifest.value()))
        else {
            return std::borrow::Cow::Borrowed(manifest);
        };
        let mut extended = manifest.clone();
        extender.apply(extended.value_mut());
        std::borrow::Cow::Owned(extended)
    }

    pub(super) fn overridden<'m>(
        &self,
        (root_dir, manifest): (&Path, &'m PackageManifest),
    ) -> std::borrow::Cow<'m, PackageManifest> {
        let Some(overrider) = &self.overrider else {
            return std::borrow::Cow::Borrowed(manifest);
        };
        let mut cloned = manifest.clone();
        overrider.apply(&mut cloned, Some(root_dir));
        std::borrow::Cow::Owned(cloned)
    }
}
