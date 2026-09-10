use super::{
    BinOrigin, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimTargetCache, link_bins,
    link_bins_of_packages, link_bins_of_packages_cached, remove_bin,
};
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
