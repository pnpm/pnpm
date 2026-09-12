//! Which artifact provides a package manager, and which packages make up
//! an installed engine once its exact version is known.
//!
//! Two families. Registry engines are npm packages: they resolve,
//! signature-verify and install through the same pipeline pnpm's own
//! engine uses. Binary engines are platform archives published by the
//! package manager's own project and pinned by a publisher checksum, like
//! the managed runtimes.

use crate::cli_args::self_update::install_pnpm::pnpm_package_to_install;
use pnpm_env_installer::pnpm_engine_packages;

/// Yarn moved its CLI to `@yarnpkg/cli-dist` in 2.0, and Yarn 6
/// (`yarnpkg/zpm`) is a native binary with no npm package at all.
const YARN_BERRY_MAJOR: u64 = 2;
const YARN_NATIVE_MAJOR: u64 = 6;

const NPM_PACKAGES: [&str; 1] = ["npm"];
const YARN_CLASSIC_PACKAGES: [&str; 1] = ["yarn"];
const YARN_BERRY_PACKAGES: [&str; 1] = ["@yarnpkg/cli-dist"];

/// A package manager pnpm can provision — for a project that pins one, or
/// for a git-hosted dependency that has to be installed with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageManager {
    Bun,
    Npm,
    Pnpm,
    Yarn,
}

/// Where a package manager's bytes come from. Selected from the version
/// specifier alone, because the specifier decides which source can resolve
/// it at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channel {
    /// An npm package. The name is what the version specifier resolves
    /// against; the installed engine may pin more packages than this one
    /// (see [`EnginePackages`]).
    Registry {
        package: &'static str,
    },
    Binary(BinaryChannel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryChannel {
    /// `oven-sh/bun` release archives — the source the managed Bun
    /// runtime already installs from.
    Bun,
    /// `yarnpkg/zpm` release archives, Yarn 6 and above.
    Yarn,
}

/// The packages an installed registry engine consists of, chosen once the
/// exact version is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnginePackages {
    /// The package whose bins are linked into the engine's bin directory.
    pub(crate) wrapper: &'static str,
    /// Every package pinned in the engine's lockfile.
    pub(crate) pinned: &'static [&'static str],
    /// Whether the wrapper carries a native binary that has to be linked
    /// in by hand, because engine installs run with scripts disabled.
    pub(crate) links_native_binary: bool,
}

impl PackageManager {
    /// The package managers a `packageManager` / `devEngines.packageManager`
    /// field can name. Anything else is not provisionable.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "bun" => Some(PackageManager::Bun),
            "npm" => Some(PackageManager::Npm),
            "pnpm" => Some(PackageManager::Pnpm),
            "yarn" => Some(PackageManager::Yarn),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            PackageManager::Bun => "bun",
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
        }
    }

    /// The bin names this package manager publishes, and therefore the
    /// shims `pnpm shim add <pm>` creates for it. Listed rather than read
    /// from a manifest because a shim is created before any version is
    /// chosen, and because the aliases differ across releases.
    pub(crate) fn bins(self) -> &'static [&'static str] {
        match self {
            // Bun's release archive holds the one executable; `bunx` is a
            // link its own installer makes, and `bun x` does the same job.
            PackageManager::Bun => &["bun"],
            PackageManager::Npm => &["npm", "npx"],
            PackageManager::Pnpm => &["pnpm", "pnpx", "pn", "pnx"],
            PackageManager::Yarn => &["yarn", "yarnpkg"],
        }
    }

    pub(crate) fn channel(self, version_spec: &str) -> Channel {
        match self {
            PackageManager::Bun => Channel::Binary(BinaryChannel::Bun),
            PackageManager::Npm => Channel::Registry { package: NPM_PACKAGES[0] },
            PackageManager::Pnpm => Channel::Registry { package: "pnpm" },
            PackageManager::Yarn => yarn_channel(version_spec),
        }
    }

    /// The engine's package set for an exact `version`. `None` for an
    /// engine that arrives as a single platform archive and therefore has
    /// no package closure.
    pub(crate) fn engine_packages(self, version: &str) -> Option<EnginePackages> {
        match self {
            PackageManager::Bun => None,
            PackageManager::Npm => Some(single_package(&NPM_PACKAGES)),
            PackageManager::Pnpm => {
                let package = pnpm_package_to_install(version);
                Some(EnginePackages {
                    wrapper: package.name,
                    pinned: pnpm_engine_packages(version),
                    links_native_binary: package.links_native_binary,
                })
            }
            PackageManager::Yarn => match yarn_channel(version) {
                Channel::Registry { package } if package == YARN_CLASSIC_PACKAGES[0] => {
                    Some(single_package(&YARN_CLASSIC_PACKAGES))
                }
                Channel::Registry { .. } => Some(single_package(&YARN_BERRY_PACKAGES)),
                Channel::Binary(_) => None,
            },
        }
    }
}

fn single_package(packages: &'static [&'static str; 1]) -> EnginePackages {
    EnginePackages { wrapper: packages[0], pinned: packages, links_native_binary: false }
}

fn yarn_channel(version_spec: &str) -> Channel {
    match committed_major(version_spec) {
        Some(major) if major >= YARN_NATIVE_MAJOR => Channel::Binary(BinaryChannel::Yarn),
        Some(major) if major < YARN_BERRY_MAJOR => {
            Channel::Registry { package: YARN_CLASSIC_PACKAGES[0] }
        }
        _ => Channel::Registry { package: YARN_BERRY_PACKAGES[0] },
    }
}

/// The lowest major a specifier admits, or `None` when it admits any
/// version — a dist-tag, a wildcard, or something unparsable. Yarn's
/// channels are split by major, so a specifier that does not commit to one
/// falls through to the current line rather than to Yarn 1.
fn committed_major(version_spec: &str) -> Option<u64> {
    let version_spec = version_spec.trim();
    if version_spec.is_empty() || version_spec == "*" || version_spec.eq_ignore_ascii_case("x") {
        return None;
    }
    Some(node_semver::Range::parse(version_spec).ok()?.min_version()?.major)
}

#[cfg(test)]
mod tests;
