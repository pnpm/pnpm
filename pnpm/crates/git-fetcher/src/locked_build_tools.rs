//! The tools the lockfile records for building a git-hosted package, which
//! the caller installs before the prepare runs.

use pnpm_cmd_shim::get_bins_from_package_manifest_with_walk;
use pnpm_package_manifest::safe_read_package_json_from_dir;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Where [`crate::prepare_package()`] gets the tools a build runs with.
#[derive(Debug, Default, Clone, Copy)]
pub enum BuildTools<'a> {
    /// The package manager the package asks for, provisioned when it pins
    /// a version or the host cannot satisfy it, and the host's Node.js.
    #[default]
    Detected,
    /// Only the tools the lockfile records (`lockBuildTools`). `None` when
    /// it records none for the package.
    Locked(Option<&'a LockedBuildTools>),
}

impl BuildTools<'_> {
    /// [`LockedBuildTools::identity`] of the recorded tools, if any.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        match self {
            BuildTools::Locked(Some(tools)) => Some(&tools.identity),
            BuildTools::Detected | BuildTools::Locked(None) => None,
        }
    }
}

/// The recorded tools of one package, installed.
#[derive(Debug, Default)]
pub struct LockedBuildTools {
    /// `None` when pnpm prepares the package: the running pnpm, or the
    /// version the package pins, does that as it does without a record.
    pub package_manager: Option<ProvisionedTool>,
    /// `None` when the package pins no Node.js for its build, which then
    /// runs on the host's.
    pub node: Option<ProvisionedTool>,
    /// Names every recorded tool by lockfile key and resolution. Part of
    /// the store-index key of the package the tools build.
    pub identity: String,
}

/// A tool installed from its lockfile entry into a directory of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionedTool {
    /// The tool's name in the lockfile (`yarn`, `node`).
    pub name: String,
    pub version: String,
    /// The commands the tool publishes, by name.
    pub bins: BTreeMap<String, PathBuf>,
    /// Whether the commands are scripts Node.js runs rather than
    /// executables of their own.
    pub runs_on_node: bool,
}

impl ProvisionedTool {
    /// Read the commands of the tool installed at `dir` from its
    /// `package.json`.
    pub fn from_dir(
        name: String,
        version: String,
        dir: &Path,
        runs_on_node: bool,
    ) -> Result<Self, pnpm_package_manifest::PackageManifestError> {
        let manifest = safe_read_package_json_from_dir(dir)?.unwrap_or_default();
        let bins =
            get_bins_from_package_manifest_with_walk(&manifest, dir, |_| Ok(Vec::<PathBuf>::new()))
                .into_iter()
                .map(|command| (command.name, command.path))
                .collect();
        Ok(ProvisionedTool { name, version, bins, runs_on_node })
    }

    /// The command `name`, or the tool's own command when it publishes no
    /// such alias (a single-executable Yarn has no `yarnpkg`).
    #[must_use]
    pub fn command(&self, name: &str) -> Option<&Path> {
        self.bins
            .get(name)
            .or_else(|| self.bins.get(&self.name))
            .map(PathBuf::as_path)
    }
}

#[cfg(test)]
mod tests;
