use super::{FsReadDir, HashSet, LinkBinsError, Path};
use crate::link_bins::shim_writer::remove_if_exists;

pub(super) fn remove_unclaimed(
    name: &str,
    bins_dir: &Path,
    provided: &HashSet<String>,
) -> Result<(), LinkBinsError> {
    for name in removable_names(name, provided, cfg!(windows)) {
        let path = bins_dir.join(name);
        remove_if_exists(&path).map_err(|error| LinkBinsError::RemoveStaleBin { path, error })?;
    }
    Ok(())
}

/// Remove any entry in `bins_dir` whose command name is not in `claimed_commands`.
pub fn prune_stale_bins(
    bins_dir: &Path,
    claimed_commands: &HashSet<String>,
) -> Result<(), LinkBinsError> {
    prune_stale_bins_with::<crate::Host>(bins_dir, claimed_commands)
}

pub fn prune_stale_bins_with<Sys: FsReadDir>(
    bins_dir: &Path,
    claimed_commands: &HashSet<String>,
) -> Result<(), LinkBinsError> {
    let entries = match Sys::read_dir(bins_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(LinkBinsError::ReadModulesDir { dir: bins_dir.to_path_buf(), error });
        }
    };
    let windows = cfg!(windows);
    let normalized_claimed: HashSet<String>;
    let provided = if windows {
        normalized_claimed = claimed_commands
            .iter()
            .map(|name| normalize(name, true))
            .collect();
        &normalized_claimed
    } else {
        claimed_commands
    };
    for path in entries {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_claimed(name, provided, windows) {
            remove_if_exists(&path).map_err(|error| LinkBinsError::RemoveStaleBin { path, error })?;
        }
    }
    Ok(())
}

pub(super) fn removable_names(
    name: &str,
    provided: &HashSet<String>,
    windows: bool,
) -> Vec<String> {
    let mut names = vec![name.to_owned()];
    if windows {
        names.extend(["cmd", "ps1", "exe"].map(|extension| format!("{name}.{extension}")));
    }
    names.retain(|name| !is_claimed(name, provided, windows));
    names
}

pub(super) fn is_claimed(name: &str, provided: &HashSet<String>, windows: bool) -> bool {
    let name = normalize(name, windows);
    provided.contains(&name)
        || windows
            && [".cmd", ".ps1", ".exe"]
                .iter()
                .any(|suffix| {
                    name.strip_suffix(suffix)
                        .is_some_and(|base| provided.contains(base))
                })
}

pub(super) fn normalize(name: &str, windows: bool) -> String {
    if windows { name.to_lowercase() } else { name.to_owned() }
}
