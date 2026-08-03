use super::bundled_node_gyp_bin_in;
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

/// Lay out the published payload under `exe_dir` the way the npm
/// wrapper package ships it.
fn ship_payload(exe_dir: &Path) {
    let bin_dir = exe_dir.join("dist").join("node-gyp-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("node-gyp"), "#!/usr/bin/env sh\n").unwrap();
    fs::write(bin_dir.join("node-gyp.cmd"), "@echo off\n").unwrap();
}

#[test]
fn finds_the_wrapper_dir_shipped_beside_the_executable() {
    let exe_dir = tempfile::tempdir().unwrap();
    ship_payload(exe_dir.path());

    assert_eq!(
        bundled_node_gyp_bin_in(exe_dir.path()),
        Some(exe_dir.path().join("dist").join("node-gyp-bin")),
    );
}

#[test]
fn absent_when_nothing_was_shipped() {
    let exe_dir = tempfile::tempdir().unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}

/// A `dist/` with no `node-gyp-bin` is what a checkout build looks like:
/// the directory is not proof the payload is there.
#[test]
fn absent_when_dist_exists_without_the_wrapper_dir() {
    let exe_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(exe_dir.path().join("dist")).unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}

/// The probe must be the wrapper file itself. An empty `node-gyp-bin`
/// would otherwise be put on `PATH`, where it resolves nothing and
/// silently shadows a working node-gyp further down.
#[test]
fn absent_when_the_wrapper_dir_is_empty() {
    let exe_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(exe_dir.path().join("dist").join("node-gyp-bin")).unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}

/// Only the wrapper this platform's `PATH` resolution will actually
/// look for counts: a payload carrying just the other platform's twin
/// resolves nothing here.
#[test]
fn absent_when_only_the_other_platforms_wrapper_was_shipped() {
    let exe_dir = tempfile::tempdir().unwrap();
    let bin_dir = exe_dir.path().join("dist").join("node-gyp-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let other = if cfg!(windows) { "node-gyp" } else { "node-gyp.cmd" };
    fs::write(bin_dir.join(other), "").unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}

/// A directory named `node-gyp` inside the wrapper dir is not a wrapper.
#[test]
fn absent_when_the_wrapper_is_a_directory() {
    let exe_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(exe_dir.path().join("dist").join("node-gyp-bin").join("node-gyp")).unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}
