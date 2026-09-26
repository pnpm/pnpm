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
    let one = std::env::temp_dir().join("one");
    let two = std::env::temp_dir().join("two");
    let first = LinkBinsOptions {
        extra_node_paths: vec![
            one.join("node_modules")
                .to_string_lossy()
                .into_owned(),
        ],
        relocatable_root: Some(one.clone()),
        installed_modules_dir: Some(one.join("node_modules")),
        ..LinkBinsOptions::default()
    };
    let second = LinkBinsOptions {
        extra_node_paths: vec![
            two.join("node_modules")
                .to_string_lossy()
                .into_owned(),
        ],
        relocatable_root: Some(two.clone()),
        installed_modules_dir: Some(two.join("node_modules")),
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
