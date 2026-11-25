use super::{
    runtime_version_matches,
    with_global_bin_lock,
};
use crate::cli_args::global_bin_lock::try_acquire_global_bin_lock;
use std::fs;
use tempfile::tempdir;

#[test]
fn version_matches_an_exact_or_prefix_version() {
    assert!(runtime_version_matches("node", "22.14.0", "22.14.0"));
    assert!(runtime_version_matches("node", "22.14.0", "22.14"));
    assert!(runtime_version_matches("bun", "1.2.3", "v1.2"));
    assert!(!runtime_version_matches("deno", "1.2.30", "1.2.3"));
}

#[test]
fn node_version_matches_a_release_channel_and_a_leading_v() {
    assert!(runtime_version_matches("node", "22.0.0-rc.4", "rc/22.0.0-rc.4"));
    assert!(runtime_version_matches("node", "22.14.0", "v22"));
    assert!(!runtime_version_matches("node", "22.0.0", "lts/18"));
}

#[test]
fn version_matches_a_caret_range_written_by_set() {
    assert!(runtime_version_matches("node", "^22.14.0", "22"));
    assert!(runtime_version_matches("node", "^22.14.0", "22.14"));
    assert!(runtime_version_matches("bun", "^1.2.3", "1"));
    assert!(runtime_version_matches("node", "^rc/22.0.0-rc.4", "rc/22"));
    assert!(!runtime_version_matches("node", "^20.0.0", "22"));
}

#[test]
fn an_empty_version_does_not_match_anything() {
    assert!(!runtime_version_matches("node", "22.14.0", ""));
    assert!(!runtime_version_matches("deno", "2.0.0", "  "));
}

#[test]
fn global_runtime_selection_holds_the_bin_lock() {
    let dir = tempdir().unwrap();
    let global_bin_dir = dir.path().join("bin");
    fs::create_dir_all(&global_bin_dir).unwrap();

    let lock_held = with_global_bin_lock(&global_bin_dir, || {
        Ok::<_, miette::Report>(try_acquire_global_bin_lock(&global_bin_dir).unwrap().is_none())
    })
    .unwrap();

    assert!(lock_held, "selection and removal must run under the global bin lock");
}
