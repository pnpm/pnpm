//! The tools a prepare runs with, put on its `PATH`.

use super::{PreparePackageOptions, describe_wanted_pm, provide_wanted_pm};
use crate::{
    BuildTools, LockedBuildTools,
    error::PreparePackageError,
    pm_shims::write_locked_pm_shims,
    preferred_pm::{PreferredPm, WantedPm},
};
use pnpm_network::redact_and_sanitize;
use pnpm_reporter::Reporter;
use std::path::{Path, PathBuf};

/// What the build's `PATH` gets on top of the caller's: the shims of the
/// package manager, and the directory of the recorded Node.js.
pub(super) struct BuildPath<'a> {
    /// Removed when dropped, taking the shims with it.
    shims_dir: Option<tempfile::TempDir>,
    pub(super) node: Option<&'a Path>,
}

impl BuildPath<'_> {
    pub(super) fn bin_paths(&self, extra_bin_paths: &[PathBuf]) -> Vec<PathBuf> {
        let shims = self.shims_dir
            .as_ref()
            .map(|dir| dir.path().to_path_buf());
        let node = self.node.and_then(Path::parent).map(Path::to_path_buf);
        shims
            .into_iter()
            .chain(extra_bin_paths.iter().cloned())
            .chain(node)
            .collect()
    }
}

pub(super) fn provide_build_tools<'a, Reporter: self::Reporter>(
    opts: &PreparePackageOptions<'a>,
    wanted_pm: &WantedPm,
    dep_path: &str,
) -> Result<BuildPath<'a>, PreparePackageError> {
    let running_pnpm = opts.scripts.running_pnpm;
    Ok(match opts.build_tools {
        BuildTools::Detected => BuildPath {
            shims_dir: provide_wanted_pm::<Reporter>(wanted_pm, dep_path, running_pnpm)?,
            node: None,
        },
        BuildTools::Locked(tools) => BuildPath {
            shims_dir: provide_locked_pm::<Reporter>(wanted_pm, tools, dep_path, running_pnpm)?,
            node: tools.and_then(locked_node),
        },
    })
}

/// The recorded Node.js the build runs on, when the lockfile records one.
fn locked_node(tools: &LockedBuildTools) -> Option<&Path> {
    tools.node.as_ref()?.command("node")
}

/// [`provide_wanted_pm`] for a package whose build tools the lockfile
/// records: the recorded package manager, and nothing from the host.
///
/// pnpm is the exception. The lockfile records no pnpm, so a package that
/// asks for pnpm is prepared the way [`provide_wanted_pm`] prepares it.
fn provide_locked_pm<Reporter: self::Reporter>(
    wanted_pm: &WantedPm,
    tools: Option<&LockedBuildTools>,
    dep_path: &str,
    running_pnpm: crate::RunningPnpm<'_>,
) -> Result<Option<tempfile::TempDir>, PreparePackageError> {
    if wanted_pm.pm == PreferredPm::Pnpm {
        return provide_wanted_pm::<Reporter>(wanted_pm, dep_path, running_pnpm);
    }
    let Some(tool) = tools.and_then(|tools| tools.package_manager.as_ref()) else {
        return Err(PreparePackageError::MissingBuildTools {
            dep_path: redact_and_sanitize(dep_path),
            package_manager: describe_wanted_pm(wanted_pm),
        });
    };
    if tool.name != wanted_pm.pm.name() {
        return Err(PreparePackageError::BuildToolMismatch {
            dep_path: redact_and_sanitize(dep_path),
            recorded: format!("{}@{}", tool.name, tool.version),
            package_manager: describe_wanted_pm(wanted_pm),
        });
    }
    let node = tools.and_then(locked_node);
    tempfile::tempdir()
        .and_then(|dir| {
            write_locked_pm_shims(dir.path(), wanted_pm.pm, tool, node)?;
            Ok(dir)
        })
        .map(Some)
        .map_err(|source| PreparePackageError::PackageManagerUnavailable {
            package_manager: format!("{}@{}", tool.name, tool.version),
            source,
        })
}
