//! Detect the package manager and version expected by a git-hosted dependency
//! from the manifest and lockfile or workspace configuration it ships.

use crate::PackageFiles;
use pnpm_package_manifest::package_manager_spec::{
    dev_engines_package_managers, engine_name_version, split_spec, version_without_build,
};
use serde_json::Value;

/// Package manager a git-hosted dep wants to install with. The variant
/// drives the synthesized `<pm>-install` script in
/// [`crate::prepare_package()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Whether the dependency explicitly declared a version pin.
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

/// Determine the package manager and version to prepare the package whose
/// repository root holds `files`.
///
/// Manifest pins (`packageManager`, `devEngines.packageManager`) take precedence
/// over lockfiles and `pnpm-workspace.yaml`. When Yarn is selected without an
/// explicit version specifier, the Yarn line is inferred from `yarn.lock`.
#[must_use]
pub fn detect_wanted_pm(
    files: &(impl PackageFiles + ?Sized),
    manifest: Option<&Value>,
) -> WantedPm {
    let wanted = manifest
        .and_then(manifest_pin)
        .unwrap_or_else(|| WantedPm {
            pm: detect_preferred_pm(files),
            version_spec: None,
            pinned: false,
        });
    if wanted.version_spec.is_some() || wanted.pm != PreferredPm::Yarn {
        return wanted;
    }
    WantedPm { version_spec: yarn_line_of_lockfile(files), ..wanted }
}

const YARN_CLASSIC_SPEC: &str = "1";
const YARN_BERRY_SPEC: &str = ">=2";

/// Sniff `files` for a lockfile or `pnpm-workspace.yaml` and return the matching package manager.
///
/// Lockfiles take precedence over `pnpm-workspace.yaml`. Defaults to [`PreferredPm::Npm`]
/// when neither is present.
#[must_use]
pub fn detect_preferred_pm(files: &(impl PackageFiles + ?Sized)) -> PreferredPm {
    if files.contains("pnpm-lock.yaml") {
        return PreferredPm::Pnpm;
    }
    if files.contains("yarn.lock") {
        return PreferredPm::Yarn;
    }
    if files.contains("package-lock.json") {
        return PreferredPm::Npm;
    }
    if files.contains("bun.lockb") || files.contains("bun.lock") {
        return PreferredPm::Bun;
    }
    if files.contains("pnpm-workspace.yaml") {
        return PreferredPm::Pnpm;
    }
    PreferredPm::Npm
}

/// Extract the first supported package manager pin declared in `manifest`.
fn manifest_pin(manifest: &Value) -> Option<WantedPm> {
    dev_engines_pins(manifest)
        .chain(package_manager_pin(manifest))
        .find_map(|(name, version_spec)| {
            let pm = PreferredPm::parse(&name)?;
            let pinned = version_spec.is_some();
            Some(WantedPm { pm, version_spec, pinned })
        })
}

fn package_manager_pin(manifest: &Value) -> Option<(String, Option<String>)> {
    let (name, reference) = split_spec(manifest.get("packageManager")?.as_str()?);
    let version = reference.map(version_without_build).and_then(pinned_version);
    Some((name.to_string(), version))
}

fn dev_engines_pins(manifest: &Value) -> impl Iterator<Item = (String, Option<String>)> {
    dev_engines_package_managers(manifest)
        .filter_map(engine_name_version)
        .map(|(name, version)| (name.to_string(), version.and_then(pinned_version)))
}

/// The version a dependency pins, kept only when it is a plain semver range.
/// Manifest input is untrusted and reaches command execution during package prepare,
/// so references naming URLs or dist-tags are rejected and left for pnpm to resolve.
fn pinned_version(version: &str) -> Option<String> {
    node_semver::Range::parse(version).is_ok().then(|| version.to_string())
}

/// Detect whether the `yarn.lock` among `files` was written by Yarn Classic or Yarn Berry.
fn yarn_line_of_lockfile(files: &(impl PackageFiles + ?Sized)) -> Option<String> {
    const HEADER_BYTES: u64 = 64 * 1024;

    let header = files.read_head("yarn.lock", HEADER_BYTES)?;
    let berry = header
        .split(|byte| *byte == b'\n')
        .any(|line| line.trim_ascii_start().starts_with(b"__metadata:"));
    Some(if berry { YARN_BERRY_SPEC } else { YARN_CLASSIC_SPEC }.to_string())
}

#[cfg(test)]
mod tests;
