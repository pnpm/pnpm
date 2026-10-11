//! What a git-hosted package's build needs, decided by its files at the
//! locked commit. Resolution records it in the lockfile and the prepare pass
//! reads the same answer back, so the two agree by construction.

use crate::{PackageFiles, PreferredPm, WantedPm, detect_wanted_pm};
use pnpm_package_manifest::{BINDING_GYP, downloadable_runtime_version};
use serde_json::Value;

/// Scripts to re-run after `<pm>-install`. `prepare` itself runs as part of
/// `<pm>-install` (npm/yarn/pnpm fold it into the install lifecycle), so it
/// is not invoked separately.
///
/// `prepublishOnly` is omitted: neither npm nor Yarn run it for git-hosted
/// deps.
pub(crate) const PREPUBLISH_SCRIPTS: &[&str] = &["prepublish", "prepack", "publish"];

/// The tools a git-hosted package is prepared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildEnvironment {
    pub package_manager: WantedPm,
    /// The Node.js range the package pins in `devEngines.runtime` with
    /// `onFail: "download"`.
    ///
    /// `None` when it pins none, when Bun prepares it, or when the package
    /// ships a `binding.gyp`: an addon compiled by the prepare has to load
    /// in the project's runtime, not in the one the package develops on.
    pub node: Option<String>,
}

/// The build environment of the package whose own directory holds `package`
/// and whose repository root holds `repository`. `None` when the package
/// needs no build.
#[must_use]
pub fn detect_build_environment(
    repository: &(impl PackageFiles + ?Sized),
    package: &(impl PackageFiles + ?Sized),
    manifest: &Value,
) -> Option<BuildEnvironment> {
    if !requires_build(manifest, package) {
        return None;
    }
    let package_manager = detect_wanted_pm(repository, Some(manifest));
    let node = (package_manager.pm != PreferredPm::Bun && !package.contains(BINDING_GYP))
        .then(|| downloadable_runtime_version(manifest, "devEngines", "node"))
        .flatten()
        .filter(|range| node_semver::Range::parse(range).is_ok())
        .map(str::to_string);
    Some(BuildEnvironment { package_manager, node })
}

/// Whether preparing the package runs any of its scripts: a `prepare`, or a
/// prepublish script that has not already produced the package's `main`.
pub(crate) fn requires_build(manifest: &Value, package: &(impl PackageFiles + ?Sized)) -> bool {
    let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
        return false;
    };
    let has_script = |name: &str| {
        scripts
            .get(name)
            .and_then(Value::as_str)
            .is_some_and(|script| !script.is_empty())
    };
    if has_script("prepare") {
        return true;
    }
    if !PREPUBLISH_SCRIPTS.iter().any(|name| has_script(name)) {
        return false;
    }
    let main_file = manifest
        .get("main")
        .and_then(Value::as_str)
        .unwrap_or("index.js");
    !package.contains(main_file)
}

#[cfg(test)]
mod tests;
