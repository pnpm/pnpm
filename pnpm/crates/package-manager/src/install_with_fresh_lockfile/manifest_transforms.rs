//! pnpm's built-in read-package hook chain: `packageExtensions` (the
//! compatibility DB plus the user's), legacy deploy's workspace
//! injection, `pnpm.overrides`, and `ignoredOptionalDependencies`.
//!
//! Owns the *transform* half of the resolve inputs. The seeds, options,
//! and reuse decisions the same resolve consumes live in
//! [`super::resolve`].

use super::{
    InstallWithFreshLockfileError, compose_manifest_hooks, parse_config_overrides,
    resolved_overrides_map,
};
use crate::{
    VersionsOverrider, apply_deploy_manifest_hook, install::apply_deploy_manifest_hook_to_arc,
};
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{
    Config,
    matcher::{Matcher, create_matcher},
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{DependencyOverrider, ManifestHook};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// pnpm's built-in read-package hook chain for the manifests fresh
/// resolution consumes, plus the pieces later phases read off it.
///
/// The order matches `createReadPackageHook`: packageExtensions first,
/// then the pnpmfile and deploy hook, then overrides, then the
/// `ignoredOptionalDependencies` removal. The hooks stay separate because
/// the resolver interleaves the pnpmfile's `readPackage` between them:
/// packageExtensions → readPackage → deploy → overrides → ignored optionals.
/// This prevents a hook that replaces the manifest from erasing the deploy
/// injection, the overrides, or the removal.
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
    deploy_manifest_hook: bool,
) -> Result<ManifestTransforms, InstallWithFreshLockfileError> {
    let parsed_overrides = parse_config_overrides(config, catalogs)?;
    let resolved_overrides = parsed_overrides.as_deref().map(resolved_overrides_map);

    let compat_package_extender = (!config.ignore_compatibility_db)
        .then(crate::compat_package_extensions::compat_package_extender);
    let package_extender = configured_package_extender(config)?;
    let package_extensions_checksum = super::compute_package_extensions_checksum(config);
    let versions_overrider = parsed_overrides
        .as_ref()
        .map(|parsed| Arc::new(VersionsOverrider::new(parsed, lockfile_dir)));

    let ignored_optional_matcher =
        create_matcher(config.ignored_optional_dependencies.as_deref().unwrap_or_default());
    let mut effective_importer_manifests = BTreeMap::new();
    if compat_package_extender.is_some()
        || package_extender.is_some()
        || versions_overrider.as_ref().is_some_and(|overrider| !overrider.is_empty())
        || deploy_manifest_hook
        || !ignored_optional_matcher.is_empty()
    {
        // Every importer's transform is independent — a clone of its own
        // manifest plus in-place rewrites — so a workspace-scale set fans
        // out across rayon. The rebuilt `BTreeMap` restores the ordering
        // regardless of completion order.
        use rayon::prelude::*;
        let transforms = ImporterTransforms {
            compat_package_extender,
            package_extender: package_extender.as_ref(),
            versions_overrider: versions_overrider.as_ref(),
            deploy_manifest_hook,
            ignored_optional_matcher: &ignored_optional_matcher,
        };
        effective_importer_manifests = importer_manifests
            .par_iter()
            .map(|(id, manifest)| (id.clone(), transform_importer_manifest(manifest, &transforms)))
            .collect();
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
    let deploy_manifest_hook: Option<ManifestHook> =
        deploy_manifest_hook.then(|| Arc::new(apply_deploy_manifest_hook_to_arc) as ManifestHook);
    let ignored_optional_hook = (!ignored_optional_matcher.is_empty()).then(|| {
        Arc::new(move |mut manifest: Arc<Value>| {
            let ignored = ignored_optional_names(&manifest, &ignored_optional_matcher);
            if !ignored.is_empty() {
                remove_ignored_dependencies(Arc::make_mut(&mut manifest), &ignored);
            }
            manifest
        }) as ManifestHook
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
        overrides_hook: [deploy_manifest_hook, overrides_hook, ignored_optional_hook]
            .into_iter()
            .fold(None, compose_manifest_hooks),
        override_bare_specifier,
        effective_importer_manifests,
    })
}

/// The user's `packageExtensions`, when they extend anything at all.
fn configured_package_extender(
    config: &Config,
) -> Result<Option<Arc<crate::PackageExtender>>, InstallWithFreshLockfileError> {
    let Some(extensions) = config.package_extensions.as_ref() else { return Ok(None) };
    let extender = crate::PackageExtender::new(extensions)
        .map_err(InstallWithFreshLockfileError::InvalidPackageExtensionSelector)?;
    Ok((!extender.is_empty()).then(|| Arc::new(extender)))
}

/// Everything that rewrites an importer's manifest before the resolve reads
/// it.
struct ImporterTransforms<'a> {
    compat_package_extender: Option<&'static crate::PackageExtender>,
    package_extender: Option<&'a Arc<crate::PackageExtender>>,
    versions_overrider: Option<&'a Arc<VersionsOverrider>>,
    deploy_manifest_hook: bool,
    ignored_optional_matcher: &'a Matcher,
}

fn transform_importer_manifest(
    manifest: &PackageManifest,
    transforms: &ImporterTransforms<'_>,
) -> PackageManifest {
    let mut cloned = manifest.clone();
    if let Some(extender) = transforms.compat_package_extender {
        extender.apply(cloned.value_mut());
    }
    if let Some(extender) = transforms.package_extender {
        extender.apply(cloned.value_mut());
    }
    if transforms.deploy_manifest_hook {
        apply_deploy_manifest_hook(cloned.value_mut());
    }
    if let Some(overrider) = transforms.versions_overrider {
        let manifest_dir = cloned.path().parent().map(Path::to_path_buf);
        overrider.apply(&mut cloned, manifest_dir.as_deref());
    }
    let ignored = ignored_optional_names(cloned.value(), transforms.ignored_optional_matcher);
    if !ignored.is_empty() {
        remove_ignored_dependencies(cloned.value_mut(), &ignored);
    }
    cloned
}

/// The `optionalDependencies` entries `ignoredOptionalDependencies` matches.
///
/// Callers query this before mutating, so a shared manifest with no match
/// skips the copy-on-write clone — the common case across the thousands of
/// dependency manifests one resolve reads.
fn ignored_optional_names(manifest: &Value, matcher: &Matcher) -> Vec<String> {
    if matcher.is_empty() {
        return Vec::new();
    }
    manifest
        .get("optionalDependencies")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(serde_json::Map::keys)
        .filter(|name| matcher.matches(name))
        .cloned()
        .collect()
}

/// Drop `ignored` from `optionalDependencies` and from `dependencies` alike.
/// A package listed in both fields is optional, so ignoring it has to clear
/// both declarations or the required one would pull it back in.
fn remove_ignored_dependencies(manifest: &mut Value, ignored: &[String]) {
    for field in ["optionalDependencies", "dependencies"] {
        if let Some(dependencies) = manifest.get_mut(field).and_then(Value::as_object_mut) {
            for name in ignored {
                dependencies.remove(name);
            }
        }
    }
}
