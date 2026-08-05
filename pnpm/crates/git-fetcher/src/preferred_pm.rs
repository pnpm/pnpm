//! Detect the package manager a git-hosted dependency expects, by
//! looking for the lockfile it ships next to its `package.json`.
//!
//! Implements the file-sniffing half of the
//! [`preferred-pm`](https://www.npmjs.com/package/preferred-pm) npm
//! package. The workspace-root walk is *not* implemented — git-hosted
//! snapshots almost always ship a lockfile at the repo root, and the
//! fall-through is `Npm`.

use std::path::Path;

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
}

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

#[cfg(test)]
mod tests;
