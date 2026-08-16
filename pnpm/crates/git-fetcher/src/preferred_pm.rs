//! Detect the package manager a git-hosted dependency expects, and at
//! which version, from the lockfile and manifest it ships.
//!
//! Implements the file-sniffing half of the
//! [`preferred-pm`](https://www.npmjs.com/package/preferred-pm) npm
//! package. The workspace-root walk is *not* implemented — git-hosted
//! snapshots almost always ship a lockfile at the repo root, and the
//! fall-through is `Npm`.

use pnpm_package_manifest::package_manager_spec::{
    dev_engines_package_managers, engine_name_version, split_spec, version_without_build,
};
use serde_json::Value;
use std::{fs, io::Read as _, path::Path};

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
/// Otherwise the lockfile names the package manager.
///
/// Either way the `yarn.lock` names the Yarn line whenever the manifest
/// does not: Classic and Berry cannot read each other's lockfiles, so
/// which one wrote it is a constraint rather than a preference, and a
/// dependency that declares only "this project uses Yarn" still has to be
/// prepared with the Yarn that can read what it ships.
#[must_use]
pub fn detect_wanted_pm(dir: &Path, manifest: Option<&Value>) -> WantedPm {
    let wanted = manifest.and_then(manifest_pin).unwrap_or_else(|| WantedPm {
        pm: detect_preferred_pm(dir),
        version_spec: None,
        pinned: false,
    });
    if wanted.version_spec.is_some() || wanted.pm != PreferredPm::Yarn {
        return wanted;
    }
    WantedPm { version_spec: yarn_line_of_lockfile(dir), ..wanted }
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
/// pnpm can provision.
///
/// A pin naming something else is not a pin pnpm can honor, so the next
/// declaration still gets a say — the next entry of a `devEngines` list,
/// which declares alternatives, then `packageManager`, and finally the
/// lockfile the dependency ships.
fn manifest_pin(manifest: &Value) -> Option<WantedPm> {
    dev_engines_pins(manifest).chain(package_manager_pin(manifest)).find_map(
        |(name, version_spec)| {
            let pm = PreferredPm::parse(&name)?;
            // A declaration that names no version pnpm can honor claims
            // nothing about which release the dependency was tested
            // against, so it is not a reason to provide one the host
            // already has.
            let pinned = version_spec.is_some();
            Some(WantedPm { pm, version_spec, pinned })
        },
    )
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

/// The version a dependency's manifest pins, kept only when it is a plain
/// semver range. The manifest is untrusted input and the version reaches a
/// command line pnpm builds for the prepare, so a reference naming a URL, a
/// dist-tag, or anything else that is not a range leaves the version open
/// for pnpm to resolve rather than being passed through.
fn pinned_version(version: &str) -> Option<String> {
    node_semver::Range::parse(version).is_ok().then(|| version.to_string())
}

/// The Yarn line that can read the `yarn.lock` in `dir`, or `None` when
/// the package ships none and no line is therefore required.
///
/// Yarn Berry stamps every lockfile it writes with a `__metadata:` key,
/// in the header — so only the head is read. The file comes out of a
/// fetched artifact, and how large that is, is not pnpm's to trust.
fn yarn_line_of_lockfile(dir: &Path) -> Option<String> {
    const HEADER_BYTES: u64 = 64 * 1024;

    // Read bytes rather than text: a lockfile is not pnpm's to validate,
    // and a stray byte no encoding claims must not decide which Yarn
    // prepares the package.
    let lockfile = fs::File::open(dir.join("yarn.lock")).ok()?;
    let mut header = Vec::new();
    lockfile.take(HEADER_BYTES).read_to_end(&mut header).ok()?;
    let berry = header
        .split(|byte| *byte == b'\n')
        .any(|line| line.trim_ascii_start().starts_with(b"__metadata:"));
    Some(if berry { YARN_BERRY_SPEC } else { YARN_CLASSIC_SPEC }.to_string())
}

#[cfg(test)]
mod tests;
