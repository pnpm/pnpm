//! `$dep-name` self-references in `overrides`.
//!
//! An override value of `$foo` means "whatever specifier the root
//! manifest declares for `foo`". The reference is resolved while config
//! is read, so every downstream consumer — the read-package hook that
//! rewrites manifests, the `overrides:` map written to
//! `pnpm-lock.yaml`, and the lockfile freshness check that compares the
//! two — works with the concrete specifier.
//!
//! The syntax is deprecated in favor of catalogs, but pnpm still
//! honors it.

use crate::workspace_yaml::LoadWorkspaceYamlError;
use indexmap::IndexMap;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use std::{collections::HashMap, path::Path};

/// The dependency groups a `$dep-name` reference may point at, in
/// merge order: a name declared in more than one of them resolves to
/// the last group's specifier. `peerDependencies` is not referenceable.
const REFERENCEABLE_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional];

/// Replace every `$dep-name` value in `overrides` with the specifier
/// the manifest at `root_dir` declares for that dependency.
///
/// A workspace root without a manifest declares no dependencies, so
/// every reference then fails to resolve. A manifest that exists but
/// cannot be read or parsed propagates its own error instead — the
/// unreadable file is what the user has to fix.
pub(crate) fn resolve_version_references(
    overrides: &mut IndexMap<String, String>,
    root_dir: &Path,
) -> Result<(), LoadWorkspaceYamlError> {
    if !overrides.values().any(|spec| spec.starts_with('$')) {
        return Ok(());
    }
    let root_manifest = match PackageManifest::from_path(root_dir.join("package.json")) {
        Ok(manifest) => Some(manifest),
        Err(PackageManifestError::NoImporterManifestFound(_)) => None,
        Err(source) => {
            return Err(LoadWorkspaceYamlError::ReadRootManifest { source: Box::new(source) });
        }
    };
    let direct_dependencies: HashMap<&str, &str> = root_manifest
        .as_ref()
        .map(|manifest| manifest.dependencies(REFERENCEABLE_GROUPS).collect())
        .unwrap_or_default();
    for spec in overrides.values_mut() {
        let Some(dependency_name) = spec.strip_prefix('$') else { continue };
        let Some(resolved) = direct_dependencies.get(dependency_name) else {
            return Err(LoadWorkspaceYamlError::CannotResolveOverrideVersion {
                spec: spec.clone(),
                dependency_name: dependency_name.to_string(),
            });
        };
        let resolved = (*resolved).to_string();
        *spec = resolved;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
