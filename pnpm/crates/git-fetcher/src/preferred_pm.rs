//! Detect the package manager a git-hosted dependency expects, and at
//! which version, from the lockfile and manifest it ships.
//!
//! Implements the file-sniffing half of the
//! [`preferred-pm`](https://www.npmjs.com/package/preferred-pm) npm
//! package. The workspace-root walk is *not* implemented — git-hosted
//! snapshots almost always ship a lockfile at the repo root, and the
//! fall-through is `Npm`.

use pacquet_package_manifest::package_manager_spec::{
    dev_engines_package_managers, engine_name_version, split_spec, version_without_build,
};
use serde_json::Value;
use std::{fs, path::Path};

/// Package manager a git-hosted dep wants to install with. The variant
/// drives the synthesized `<pm>-install` script in
/// [`crate::prepare_package()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferredPm {
    Pnpm,
    Npm,
    Yarn,
    Bun,
}

/// The package manager to prepare a git-hosted dependency with, and the
/// version specifier pnpm should provision it at. `None` leaves the
/// version to the channel's own default, which is the current line of
/// that package manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedPm {
    pub pm: PreferredPm,
    pub version_spec: Option<String>,
    /// Whether the dependency asked for this version itself. A pin is
    /// what its authors test against, so it is provisioned even on a host
    /// that already has that package manager; an inferred version only
    /// applies when pnpm has to provide the package manager anyway.
    pub pinned: bool,
}

impl PreferredPm {
    /// Binary name to invoke (also the prefix of the synthesized
    /// script name written into the manifest).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            PreferredPm::Pnpm => "pnpm",
            PreferredPm::Npm => "npm",
            PreferredPm::Yarn => "yarn",
            PreferredPm::Bun => "bun",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "pnpm" => Some(PreferredPm::Pnpm),
            "npm" => Some(PreferredPm::Npm),
            "yarn" => Some(PreferredPm::Yarn),
            "bun" => Some(PreferredPm::Bun),
            _ => None,
        }
    }
}

/// Decide which package manager, at which version, prepares the package
/// checked out at `dir`.
///
/// A `packageManager` / `devEngines.packageManager` pin in the
/// dependency's own manifest wins — it is what its authors test against.
/// Otherwise the lockfile names the package manager, and for Yarn it also
/// names the line: Classic and Berry cannot read each other's lockfiles,
/// so which one a `yarn.lock` came from is a constraint, not a preference.
#[must_use]
pub fn detect_wanted_pm(dir: &Path, manifest: Option<&Value>) -> WantedPm {
    if let Some(wanted) = manifest.and_then(manifest_pin) {
        return wanted;
    }
    let pm = detect_preferred_pm(dir);
    let version_spec = (pm == PreferredPm::Yarn)
        .then(|| if ships_berry_lockfile(dir) { YARN_BERRY_SPEC } else { YARN_CLASSIC_SPEC })
        .map(ToString::to_string);
    WantedPm { pm, version_spec, pinned: false }
}

/// Yarn Berry rewrote the lockfile format, so a `yarn.lock` without its
/// `__metadata` block was written by Yarn Classic and only Classic can
/// read it — and one carrying that block needs Berry.
const YARN_CLASSIC_SPEC: &str = "1";
const YARN_BERRY_SPEC: &str = ">=2";

/// Sniff `dir` for a lockfile and return the matching package manager.
/// Defaults to [`PreferredPm::Npm`] when no lockfile is present.
#[must_use]
pub fn detect_preferred_pm(dir: &Path) -> PreferredPm {
    if dir.join("pnpm-lock.yaml").exists() {
        return PreferredPm::Pnpm;
    }
    if dir.join("yarn.lock").exists() {
        return PreferredPm::Yarn;
    }
    if dir.join("package-lock.json").exists() {
        return PreferredPm::Npm;
    }
    if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() {
        return PreferredPm::Bun;
    }
    PreferredPm::Npm
}

/// The package manager the dependency pins for itself, if it pins one
/// pnpm can provision. A pin naming something else is not a pin pnpm can
/// honor, so the next declaration — and then the lockfile — still gets a
/// say.
fn manifest_pin(manifest: &Value) -> Option<WantedPm> {
    [dev_engines_pin(manifest), package_manager_pin(manifest)].into_iter().flatten().find_map(
        |(name, version_spec)| {
            Some(WantedPm { pm: PreferredPm::parse(&name)?, version_spec, pinned: true })
        },
    )
}

fn package_manager_pin(manifest: &Value) -> Option<(String, Option<String>)> {
    let (name, reference) = split_spec(manifest.get("packageManager")?.as_str()?);
    let version = reference.map(version_without_build).and_then(pinned_version);
    Some((name.to_string(), version))
}

fn dev_engines_pin(manifest: &Value) -> Option<(String, Option<String>)> {
    let (name, version) = engine_name_version(dev_engines_package_managers(manifest).next()?)?;
    Some((name.to_string(), version.and_then(pinned_version)))
}

/// The version a dependency's manifest pins, kept only when it is a plain
/// semver range. The manifest is untrusted input and the version reaches a
/// command line pnpm builds for the prepare, so a reference naming a URL, a
/// dist-tag, or anything else that is not a range leaves the version open
/// for pnpm to resolve rather than being passed through.
fn pinned_version(version: &str) -> Option<String> {
    node_semver::Range::parse(version).is_ok().then(|| version.to_string())
}

/// Whether the `yarn.lock` in `dir` was written by Yarn Berry, which
/// stamps every lockfile with a `__metadata` block.
fn ships_berry_lockfile(dir: &Path) -> bool {
    fs::read_to_string(dir.join("yarn.lock")).is_ok_and(|lockfile| {
        lockfile.lines().any(|line| line.trim_start().starts_with("__metadata"))
    })
}

#[cfg(test)]
mod tests;
