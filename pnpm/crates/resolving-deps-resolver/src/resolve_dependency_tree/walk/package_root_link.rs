//! A relative `file:` dependency that a package from a tarball or the
//! registry declares points inside that package, which has no directory to
//! resolve the path against until it is placed on disk. The dependency is
//! rewritten to a `link:<root>/...` reference (see
//! [`pnpm_lockfile::PACKAGE_ROOT_LINK_BASE`]) and every linker resolves
//! `<root>` against the declaring package's directory.

use super::{Arc, LockfileResolution, Value, WantedDependency};
use pnpm_local_spec::is_tarball_filename;
use pnpm_lockfile::{DirectoryResolution, PACKAGE_ROOT_LINK_BASE, package_root_link_target};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult, ResolvedPackageInfo};

/// Rewrite the relative `file:` dependencies of a package from a tarball or
/// the registry that stay inside that package into `link:<root>/...`
/// references. It runs on the published manifest, before any
/// `readPackage` hook or override, so a `file:` specifier a hook writes
/// keeps its usual meaning. The `Arc` is cloned only when a specifier
/// changes.
pub(super) fn link_file_deps_inside_package(manifest: Arc<Value>) -> Arc<Value> {
    let rewrites: Vec<(&'static str, String, String)> = ["dependencies", "optionalDependencies"]
        .into_iter()
        .flat_map(|field| package_root_links(&manifest, field))
        .collect();
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

fn package_root_links<'m>(
    manifest: &'m Value,
    field: &'static str,
) -> impl Iterator<Item = (&'static str, String, String)> + 'm {
    manifest
        .get(field)
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(move |(alias, spec)| {
            let link = file_spec_to_package_root_link(spec.as_str()?)?;
            Some((field, alias.clone(), link))
        })
}

/// The `link:<root>/...` reference for a `file:` specifier whose path stays
/// inside the declaring package. A tarball target, an absolute or
/// home-relative path, the package's own directory, and a path that leaves
/// the package return `None`.
pub(super) fn file_spec_to_package_root_link(spec: &str) -> Option<String> {
    let target = spec.strip_prefix("file:")?.replace('\\', "/");
    let is_absolute = target.starts_with('/')
        || target.starts_with('~')
        || target
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':');
    if target.is_empty() || is_absolute || is_tarball_filename(&target) {
        return None;
    }
    let link_target = format!("{PACKAGE_ROOT_LINK_BASE}{}", normalize_inside(&target)?);
    package_root_link_target(&link_target)?;
    Some(format!("link:{link_target}"))
}

/// Resolve `.` and `..` segments, returning `None` when the path names the
/// starting directory itself or leaves it.
fn normalize_inside(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            _ => segments.push(segment),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
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
