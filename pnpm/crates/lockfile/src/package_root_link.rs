//! A relative `file:` dependency that a package from a tarball or the
//! registry declares points inside that package, which has no directory to
//! resolve the path against until it is placed on disk. The dependency is
//! rewritten to a `link:<root>/...` reference (see [`PACKAGE_ROOT_LINK_BASE`])
//! and every linker resolves `<root>` against the declaring package's
//! directory.

use serde_json::Value;

use crate::{
    PACKAGE_ROOT_LINK_BASE,
    package_root_link_target,
};

/// The `(field, alias, link)` rewrites that turn the relative `file:`
/// dependencies of a package from a tarball or the registry, which stay
/// inside that package, into `link:<root>/...` references. The rewrite runs on
/// the published manifest, before any `readPackage` hook or override, so a
/// `file:` specifier a hook writes keeps its usual meaning.
#[must_use]
pub fn package_root_link_rewrites(manifest: &Value) -> Vec<(&'static str, String, String)> {
    ["dependencies", "optionalDependencies"]
        .into_iter()
        .flat_map(|field| {
            manifest
                .get(field)
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(move |(alias, spec)| {
                    let link = file_spec_to_package_root_link(spec.as_str()?)?;
                    Some((field, alias.clone(), link))
                })
        })
        .collect()
}

/// Apply [`package_root_link_rewrites`] to `manifest` in place.
pub fn link_file_deps_inside_manifest(manifest: &mut Value) {
    for (field, alias, link) in package_root_link_rewrites(manifest) {
        manifest[field][alias] = Value::String(link);
    }
}

/// The `link:<root>/...` reference for a `file:` specifier whose path stays
/// inside the declaring package. A tarball target, an absolute or
/// home-relative path, the package's own directory, and a path that leaves
/// the package return `None`.
#[must_use]
pub fn file_spec_to_package_root_link(spec: &str) -> Option<String> {
    let target = spec.strip_prefix("file:")?.replace('\\', "/");
    let is_absolute = target.starts_with('/')
        || target.starts_with('~')
        || target
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':');
    if target.is_empty() || is_absolute || pnpm_local_spec::is_tarball_filename(&target) {
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
