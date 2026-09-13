use crate::safe_read_package_json_from_dir;
use serde_json::Value;
use std::path::Path;

/// Decide whether a package directory needs a build pass.
///
/// True when the package's manifest declares any of `preinstall`, `install`,
/// or `postinstall`, or when the package contains `binding.gyp` or a `.hooks/`
/// directory. Missing manifests, IO errors, and parse errors all collapse to
/// `false` — pacquet cannot meaningfully build a package whose extracted
/// content cannot be inspected.
#[must_use]
pub fn pkg_requires_build(pkg_root: &Path) -> bool {
    if pkg_root.join("binding.gyp").exists() || pkg_root.join(".hooks").is_dir() {
        return true;
    }
    let Ok(Some(manifest)) = safe_read_package_json_from_dir(pkg_root) else {
        return false;
    };
    manifest_requires_build(&manifest)
}

/// Decide whether a parsed manifest declares lifecycle scripts that
/// make its package a build candidate.
///
/// A script has to carry a value to count. An empty `postinstall` runs
/// nothing, and pnpm v11's `pkgRequiresBuild` reads the same manifest as
/// build-free, so treating the key's presence as build work would ask the
/// user to approve a build that does not exist.
#[must_use]
pub fn manifest_requires_build(manifest: &Value) -> bool {
    manifest
        .get("scripts")
        .and_then(Value::as_object)
        .is_some_and(|scripts| {
            ["preinstall", "install", "postinstall"]
                .iter()
                .any(|name| scripts.get(*name).is_some_and(script_is_set))
        })
}

/// Whether a `scripts` entry holds something to run.
///
/// Mirrors `Boolean(manifest.scripts.postinstall)` in pnpm v11's
/// `pkgRequiresBuild`: `null`, `false`, `0`, and `""` are the falsy values
/// a manifest can carry there.
fn script_is_set(script: &Value) -> bool {
    match script {
        Value::String(script) => !script.is_empty(),
        Value::Null | Value::Bool(false) => false,
        Value::Number(number) => number.as_f64() != Some(0.0),
        _ => true,
    }
}

/// Decide whether a store-index file key implies build hooks.
#[must_use]
pub fn file_path_requires_build(filename: &str) -> bool {
    filename == "binding.gyp"
        || filename
            .strip_prefix(".hooks")
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
}

#[must_use]
pub fn files_include_install_scripts<Filenames, Filename>(filenames: Filenames) -> bool
where
    Filenames: IntoIterator<Item = Filename>,
    Filename: AsRef<str>,
{
    filenames
        .into_iter()
        .any(|filename| file_path_requires_build(filename.as_ref()))
}
