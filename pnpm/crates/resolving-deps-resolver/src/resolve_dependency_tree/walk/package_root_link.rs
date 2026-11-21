//! Resolution of the `link:<root>/...` edges that
//! [`pnpm_lockfile::package_root_link_rewrites`] produces.

use super::{
    Arc,
    LockfileResolution,
    Value,
    WantedDependency,
};
use pnpm_lockfile::{
    DirectoryResolution,
    package_root_link_rewrites,
    package_root_link_target,
};
use pnpm_resolving_resolver_base::{
    PkgResolutionId,
    ResolveResult,
    ResolvedPackageInfo,
};

/// [`pnpm_lockfile::package_root_link_rewrites`] applied to a resolved
/// manifest. The `Arc` is cloned only when a specifier changes.
pub(super) fn link_file_deps_inside_package(manifest: Arc<Value>) -> Arc<Value> {
    let rewrites = package_root_link_rewrites(&manifest);
    if rewrites.is_empty() {
        return manifest;
    }
    let mut manifest = manifest;
    let fields = Arc::make_mut(&mut manifest);
    for (field, alias, link) in rewrites {
        fields[field][alias] = Value::String(link);
    }
    manifest
}

/// The path inside the declaring package that a `link:<root>/...` edge points
/// to, or `None` for any other edge.
pub(super) fn wanted_package_root_link(wanted: &WantedDependency) -> Option<&str> {
    let spec = wanted.bare_specifier.as_deref()?;
    package_root_link_target(spec.strip_prefix("link:")?)
}

/// The resolution of a `link:<root>/...` edge, built without running the
/// resolver chain or reading the target: its files are only on disk once the
/// declaring package is placed. `None` for any other specifier.
pub(super) fn package_root_link_result(wanted: &WantedDependency) -> Option<ResolveResult> {
    let target = wanted_package_root_link(wanted)?;
    let spec = wanted.bare_specifier.as_deref()?;
    let alias = wanted.alias
        .clone()
        .unwrap_or_else(|| {
            target
                .rsplit('/')
                .next()
                .unwrap_or(target)
                .to_string()
        });
    Some(ResolveResult {
        id: PkgResolutionId::from(spec.to_string()),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: target.to_string(),
        }),
        resolved_via: "local-filesystem".to_string(),
        normalized_bare_specifier: None,
        alias: Some(alias.clone()),
        policy_violation: None,
        package: ResolvedPackageInfo {
            manifest: Some(Arc::new(serde_json::json!({ "name": alias }))),
            ..ResolvedPackageInfo::default()
        },
    })
}

#[cfg(test)]
mod tests;
