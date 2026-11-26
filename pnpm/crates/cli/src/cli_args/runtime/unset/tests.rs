use super::with_global_bin_lock;
use crate::cli_args::global_bin_lock::try_acquire_global_bin_lock;
use std::fs;
use tempfile::tempdir;

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
