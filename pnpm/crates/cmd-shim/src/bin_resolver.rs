pub use pnpm_package_manifest::{PackageBin, is_safe_bin_name};

use crate::capabilities::FsWalkFiles;
use pnpm_fs::is_subdir;
use pnpm_package_manifest::{
    ScriptRuntime, declared_bin_entries, normalize_package_relative_path, unscoped_bin_name,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// One bin entry resolved from a package's `package.json`.
///
/// `name` is the command name as it should appear under `node_modules/.bin/`.
/// `path` is the absolute path to the script the shim invokes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub path: PathBuf,
    /// Precalculated script runtime (and target existence guarantee) from
    /// archive inspection or `dist.pnpr.bins`. `None` when resolved from
    /// `package.json` without precalculated bin metadata.
    pub precalculated_runtime: Option<Option<ScriptRuntime>>,
}

/// Convert precalculated [`PackageBin`] entries into [`Command`]s rooted at
/// `pkg_path`, returning `None` if any entry fails name or path validation.
#[must_use]
pub fn commands_from_precalculated_bins(
    bins: &BTreeMap<String, PackageBin>,
    pkg_path: &Path,
) -> Option<Vec<Command>> {
    let mut commands = Vec::with_capacity(bins.len());
    for (name, bin) in bins {
        if !is_safe_bin_name(name) || !bin.is_valid() {
            return None;
        }
        let rel_path = normalize_package_relative_path(&bin.path).filter(|rel| !rel.is_empty())?;
        let bin_path = pkg_path.join(&rel_path);
        if !is_subdir(pkg_path, &bin_path) {
            return None;
        }
        commands.push(Command {
            name: name.clone(),
            path: bin_path,
            precalculated_runtime: Some(bin.script_runtime()),
        });
    }
    Some(commands)
}

/// Bin names that legitimately ship inside a different package than their own
/// name.
///
/// Used by [`pkg_owns_bin`] for conflict resolution between two packages
/// declaring the same bin name.
const BIN_OWNER_OVERRIDES: &[(&str, &[&str])] = &[
    ("npx", &["npm"]),
    ("pn", &["pnpm", "@pnpm/exe"]),
    ("pnpm", &["@pnpm/exe"]),
    ("pnpx", &["pnpm", "@pnpm/exe"]),
    ("pnx", &["pnpm", "@pnpm/exe"]),
];

/// Whether `pkg_name` is a legitimate owner of the given `bin_name`. The
/// default rule is "the package named `X` owns the `X` bin"; overrides cover
/// cases like `npx` shipping inside `npm`.
#[must_use]
pub fn pkg_owns_bin(bin_name: &str, pkg_name: &str) -> bool {
    if bin_name == pkg_name {
        return true;
    }
    BIN_OWNER_OVERRIDES
        .iter()
        .find(|(name, _)| *name == bin_name)
        .is_some_and(|(_, owners)| owners.contains(&pkg_name))
}

/// Read every bin declared by `manifest` and return them as [`Command`]s
/// rooted at `pkg_path`.
///
/// An empty-string `bin` declares no command, as it does in pnpm v11.
#[must_use]
pub fn get_bins_from_package_manifest<Sys: FsWalkFiles>(
    manifest: &Value,
    pkg_path: &Path,
) -> Vec<Command> {
    get_bins_from_package_manifest_with_walk(manifest, pkg_path, |directory| {
        Sys::walk_files(directory).map(Iterator::collect::<Vec<_>>)
    })
}

/// Resolve bins with a caller-provided recursive file walk, including virtual files.
pub fn get_bins_from_package_manifest_with_walk<Paths: IntoIterator<Item = PathBuf>>(
    manifest: &Value,
    pkg_path: &Path,
    walk_files: impl FnOnce(&Path) -> std::io::Result<Paths>,
) -> Vec<Command> {
    let pkg_name = manifest.get("name").and_then(Value::as_str);
    if let Some(bin) = manifest
        .get("bin")
        .filter(|bin| bin.as_str() != Some(""))
    {
        return commands_from_bin(bin, pkg_name, pkg_path);
    }
    if let Some(bin_dir_rel) = manifest
        .get("directories")
        .and_then(|d| d.get("bin"))
        .and_then(Value::as_str)
    {
        return commands_from_directories_bin(bin_dir_rel, pkg_path, walk_files);
    }
    Vec::new()
}

/// Walk every regular file under `<pkg_path>/<bin_dir_rel>` and emit one
/// [`Command`] per file, the `directories.bin` branch of bin resolution.
///
/// Symlinks are not followed.
fn commands_from_directories_bin<Paths: IntoIterator<Item = PathBuf>>(
    bin_dir_rel: &str,
    pkg_path: &Path,
    walk_files: impl FnOnce(&Path) -> std::io::Result<Paths>,
) -> Vec<Command> {
    let bin_dir = pkg_path.join(bin_dir_rel);
    if !is_subdir(pkg_path, &bin_dir) {
        return Vec::new();
    }
    // Treat a top-level walk error as "no bins". The trait's production
    // impl already drops per-entry errors inside its iterator, so an `Err`
    // here only fires when the walker can't even open `bin_dir`.
    let Ok(paths) = walk_files(&bin_dir) else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    for path in paths {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        // Same URL-safe-name guard as the keyed-bin path.
        if !is_safe_bin_name(name) {
            continue;
        }
        commands.push(Command { name: name.to_string(), path, precalculated_runtime: None });
    }
    commands
}

fn commands_from_bin(bin: &Value, pkg_name: Option<&str>, pkg_path: &Path) -> Vec<Command> {
    let entries = declared_bin_entries(bin, pkg_name);
    let mut commands = Vec::with_capacity(entries.len());
    for (command_name, bin_relative_path) in entries {
        let bin_name = unscoped_bin_name(command_name);
        if !is_safe_bin_name(&bin_name) {
            continue;
        }
        let bin_path = pkg_path.join(&bin_relative_path);
        if !is_subdir(pkg_path, &bin_path) {
            continue;
        }
        commands.push(Command { name: bin_name, path: bin_path, precalculated_runtime: None });
    }
    commands
}

#[cfg(test)]
mod tests;
