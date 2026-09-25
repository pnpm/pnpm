use super::{
    BinOrigin, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimTargetCache,
    bin_layout_fingerprint, link_bins, link_bins_of_packages, link_bins_of_packages_cached,
    remove_bin,
};
#[cfg(unix)]
use crate::shim::is_sh_shim_hardened;
use crate::{
    capabilities::{
        DirCreation, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead,
        FsReadToString, FsSetExecutable, FsWalkFiles, FsWrite, Host,
    },
    shim::is_shim_pointing_at,
};
use serde_json::{Value, json};
use std::{
    fs::{create_dir_all, read as read_file, read_to_string, write as write_file},
    io,
    iter::{Empty, empty},
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::tempdir;

#[test]
fn bin_layout_fingerprint_ignores_cache_local_paths() {
    let first = LinkBinsOptions {
        extra_node_paths: vec!["/tmp/one/node_modules".to_string()],
        relocatable_root: Some(PathBuf::from("/tmp/one")),
        installed_modules_dir: Some(PathBuf::from("/tmp/one/node_modules")),
        ..LinkBinsOptions::default()
    };
    let second = LinkBinsOptions {
        extra_node_paths: vec!["/tmp/two/node_modules".to_string()],
        relocatable_root: Some(PathBuf::from("/tmp/two")),
        installed_modules_dir: Some(PathBuf::from("/tmp/two/node_modules")),
        ..LinkBinsOptions::default()
    };

    assert_eq!(bin_layout_fingerprint(&first), bin_layout_fingerprint(&second));
}

#[cfg(windows)]
mod windows_native;

mod runtime_removing_bin_entries_preserves;

mod runtime_resolved_location_matches_canonicalize;

mod files;

mod behavior;

mod dependencies;

mod manifests;

mod workspace_settings;

mod security;

#[cfg(unix)]
mod relocatable;

#[cfg(unix)]
mod relocation_validation;

#[cfg(unix)]
mod permissions;
