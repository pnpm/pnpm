//! pnpm's built-in read-package hook chain: `packageExtensions` (the
//! compatibility DB plus the user's) and `pnpm.overrides`.
//!
//! Owns the *transform* half of the resolve inputs. The seeds, options,
//! and reuse decisions the same resolve consumes live in
//! [`super::resolve`].

use super::{
    InstallWithFreshLockfileError, compose_manifest_hooks, parse_config_overrides,
    resolved_overrides_map,
};
use crate::VersionsOverrider;
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{DependencyOverrider, ManifestHook};
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// pnpm's built-in read-package hook chain for the manifests fresh
/// resolution consumes, plus the pieces later phases read off it.
///
/// The order matches `createReadPackageHook`: packageExtensions first,
/// overrides after. The two halves stay separate hooks because the
/// resolver interleaves the pnpmfile's `readPackage` between them —
/// packageExtensions → readPackage → overrides — so a hook that replaces
/// the manifest cannot erase the overrides.
pub(super) struct ManifestTransforms {
    pub parsed_overrides: Option<Vec<pnpm_config_parse_overrides::VersionOverride>>,
    pub resolved_overrides: Option<IndexMap<String, String>>,
    pub package_extensions_checksum: Option<String>,
    pub versions_overrider: Option<Arc<VersionsOverrider>>,
    pub manifest_hook: Option<ManifestHook>,
    pub overrides_hook: Option<ManifestHook>,
    pub override_bare_specifier: Option<Arc<DependencyOverrider>>,
    /// Importer manifests with every transform already applied. Empty
    /// when nothing transforms them, in which case the caller keeps
    /// resolving against the originals.
    pub effective_importer_manifests: BTreeMap<String, PackageManifest>,
}

pub(super) fn build_manifest_transforms(
    config: &Config,
    catalogs: &Catalogs,
    lockfile_dir: &Path,
    importer_manifests: &BTreeMap<String, &PackageManifest>,
) -> Result<ManifestTransforms, InstallWithFreshLockfileError> {
    let parsed_overrides = parse_config_overrides(config, catalogs)?;
    let resolved_overrides = parsed_overrides.as_deref().map(resolved_overrides_map);

    let compat_package_extender = if config.ignore_compatibility_db {
        None
    } else {
        Some(crate::compat_package_extensions::compat_package_extender())
    };
    let package_extender = match config.package_extensions.as_ref() {
        Some(extensions) => {
            let extender = crate::PackageExtender::new(extensions)
                .map_err(InstallWithFreshLockfileError::InvalidPackageExtensionSelector)?;
            (!extender.is_empty()).then(|| Arc::new(extender))
        }
        None => None,
    };
    let package_extensions_checksum = super::compute_package_extensions_checksum(config);
    let versions_overrider = parsed_overrides
        .as_ref()
        .map(|parsed| Arc::new(VersionsOverrider::new(parsed, lockfile_dir)));

    let mut effective_importer_manifests = BTreeMap::new();
    if compat_package_extender.is_some()
        || package_extender.is_some()
        || versions_overrider.as_ref().is_some_and(|overrider| !overrider.is_empty())
    {
        for (id, manifest) in importer_manifests {
            let mut cloned = (*manifest).clone();
            if let Some(extender) = compat_package_extender {
                extender.apply(cloned.value_mut());
            }
            if let Some(extender) = package_extender.as_ref() {
                extender.apply(cloned.value_mut());
            }
            if let Some(overrider) = versions_overrider.as_ref() {
                let manifest_dir = cloned.path().parent().map(Path::to_path_buf);
                overrider.apply(&mut cloned, manifest_dir.as_deref());
            }
            effective_importer_manifests.insert(id.clone(), cloned);
        }
    }

    let compat_package_extensions_hook: Option<ManifestHook> = compat_package_extender
        .map(|extender| Arc::new(move |manifest| extender.apply_to_arc(manifest)) as ManifestHook);
    let package_extensions_hook: Option<ManifestHook> = package_extender.as_ref().map(|extender| {
        let extender = Arc::clone(extender);
        Arc::new(move |manifest| extender.apply_to_arc(manifest)) as ManifestHook
    });
    // An empty overrider would install a hook that rewrites nothing, so
    // both sinks share the same non-empty precondition.
    let active_overrider = versions_overrider.as_ref().filter(|overrider| !overrider.is_empty());
    let overrides_hook: Option<ManifestHook> = active_overrider.map(|overrider| {
        let overrider = Arc::clone(overrider);
        Arc::new(move |manifest| overrider.apply_to_arc(manifest, None)) as ManifestHook
    });
    let override_bare_specifier: Option<Arc<DependencyOverrider>> =
        active_overrider.map(|overrider| {
            let overrider = Arc::clone(overrider);
            Arc::new(move |name: &str, range: &str, pkg_dir: &Path| {
                overrider.override_for_undeclared_dependency(name, range, pkg_dir)
            }) as Arc<DependencyOverrider>
        });

    Ok(ManifestTransforms {
        parsed_overrides,
        resolved_overrides,
        package_extensions_checksum,
        versions_overrider,
        manifest_hook: compose_manifest_hooks(
            compat_package_extensions_hook,
            package_extensions_hook,
        ),
        overrides_hook,
        override_bare_specifier,
        effective_importer_manifests,
    })
}
