//! Detect the package manager a git-hosted dependency expects, and at
//! which version, from the lockfile and manifest it ships.
//!
//! Implements the file-sniffing half of the
//! [`preferred-pm`](https://www.npmjs.com/package/preferred-pm) npm
//! package. The workspace-root walk is *not* implemented — git-hosted
//! snapshots almost always ship a lockfile at the repo root, and the
//! fall-through is `Npm`.

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
/// Otherwise the lockfile names the package manager, and the version is
/// left open except for Yarn, whose Classic and Berry lines are different
/// enough that the wrong one cannot install the other's lockfile.
#[must_use]
pub fn detect_wanted_pm(dir: &Path, manifest: Option<&Value>) -> WantedPm {
    if let Some(wanted) = manifest.and_then(manifest_pin) {
        return wanted;
    }
    let pm = detect_preferred_pm(dir);
    let version_spec = (pm == PreferredPm::Yarn && !ships_berry_lockfile(dir))
        .then(|| YARN_CLASSIC_SPEC.to_string());
    WantedPm { pm, version_spec, pinned: false }
}

/// Yarn Berry rewrote the lockfile format, so a `yarn.lock` without its
/// `__metadata` block was written by Yarn Classic and only Classic can
/// read it.
const YARN_CLASSIC_SPEC: &str = "1";

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
/// pnpm can provision.
fn manifest_pin(manifest: &Value) -> Option<WantedPm> {
    let (name, version) = dev_engines_pin(manifest).or_else(|| package_manager_pin(manifest))?;
    Some(WantedPm { pm: PreferredPm::parse(&name)?, version_spec: version, pinned: true })
}

fn package_manager_pin(manifest: &Value) -> Option<(String, Option<String>)> {
    let package_manager = manifest.get("packageManager")?.as_str()?;
    // `<name>@<version>[+<integrity>]`. A reference holding a `:` is a URL
    // rather than a version — pnpm resolves the version itself, and a
    // dependency's manifest is untrusted input, so anything that is not a
    // plain version leaves the version open.
    let (name, reference) = package_manager.split_once('@')?;
    let version = reference.split_once('+').map_or(reference, |(version, _)| version);
    let version = (!version.is_empty() && !version.contains(':')).then(|| version.to_string());
    Some((name.to_string(), version))
}

fn dev_engines_pin(manifest: &Value) -> Option<(String, Option<String>)> {
    let value = manifest.get("devEngines")?.get("packageManager")?;
    let entry = match value {
        Value::Array(entries) => entries.first()?,
        other => other,
    };
    let name = entry.get("name")?.as_str()?.to_string();
    let version = entry
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty() && !version.contains(':'))
        .map(ToString::to_string);
    Some((name, version))
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
