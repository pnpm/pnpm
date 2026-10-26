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

/// Remove any pnpm-created shim or symlink in `bins_dir` whose command name is
/// not in `claimed_commands`.
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
    if is_symlink_or_junction(bins_dir) {
        return Ok(());
    }
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
        if !is_claimed(name, provided, windows)
            && !is_symlink_or_junction(bins_dir)
            && is_pnpm_bin_entry(&path)
        {
            remove_if_exists(&path).map_err(|error| LinkBinsError::RemoveStaleBin { path, error })?;
        }
    }
    Ok(())
}

fn is_symlink_or_junction(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
        || pnpm_fs::read_symlink_dir(path).is_ok()
}

fn is_pnpm_bin_entry(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    if !meta.file_type().is_file() {
        return false;
    }
    if cfg!(windows)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("node.exe"))
    {
        return true;
    }
    let mut head = [0u8; 128];
    let Ok(read) = crate::read_head_filled::<crate::Host>(path, &mut head) else {
        return false;
    };
    let prefix = head[..read]
        .strip_prefix(b"\xEF\xBB\xBF")
        .unwrap_or(&head[..read]);
    prefix.starts_with(b"#!/bin/sh\n# Resolve $0 through symlinks")
        || prefix.starts_with(b"#!/bin/sh\nbasedir=")
        || prefix.starts_with(b"@SETLOCAL\r\n")
        || prefix.starts_with(b"@IF EXIST")
        || prefix.starts_with(b"#!/usr/bin/env pwsh\n$basedir=")
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
