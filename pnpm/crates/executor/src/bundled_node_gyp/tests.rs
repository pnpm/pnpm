use super::{bundled_node_gyp_bin_in, node_gyp_entry_in_payload};
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
        Some(
            exe_dir
                .path()
                .join("dist")
                .join("node-gyp-bin")
        ),
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
    fs::create_dir_all(
        exe_dir
            .path()
            .join("dist")
            .join("node-gyp-bin"),
    )
    .unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}

/// Only the wrapper this platform's `PATH` resolution will actually
/// look for counts: a payload carrying just the other platform's twin
/// resolves nothing here.
#[test]
fn absent_when_only_the_other_platforms_wrapper_was_shipped() {
    let exe_dir = tempfile::tempdir().unwrap();
    let bin_dir = exe_dir
        .path()
        .join("dist")
        .join("node-gyp-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let other = if cfg!(windows) { "node-gyp" } else { "node-gyp.cmd" };
    fs::write(bin_dir.join(other), "").unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}

/// A directory named `node-gyp` inside the wrapper dir is not a wrapper.
#[test]
fn absent_when_the_wrapper_is_a_directory() {
    let exe_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(
        exe_dir
            .path()
            .join("dist")
            .join("node-gyp-bin")
            .join("node-gyp"),
    )
    .unwrap();

    assert_eq!(bundled_node_gyp_bin_in(exe_dir.path()), None);
}

/// `node_modules/.bin/pnpm` and `npm install -g`'s `<prefix>/bin/pnpm` are
/// symlinks to the executable inside the package.
#[cfg(unix)]
#[test]
fn finds_the_wrapper_dir_beside_the_symlink_target() {
    use super::bundled_node_gyp_bin_beside;

    let package_dir = tempfile::tempdir().unwrap();
    ship_payload(package_dir.path());
    let exe = package_dir.path().join("pnpm");
    fs::write(&exe, "").unwrap();
    let link_dir = tempfile::tempdir().unwrap();
    let link = link_dir.path().join("pnpm");
    std::os::unix::fs::symlink(&exe, &link).unwrap();

    assert_eq!(
        bundled_node_gyp_bin_beside(&link),
        Some(
            dunce::canonicalize(package_dir.path())
                .unwrap()
                .join("dist")
                .join("node-gyp-bin")
        ),
    );
}

/// Lay out the `node-gyp.js` entry the wrapper falls back to.
fn ship_entry(exe_dir: &Path) {
    let entry_dir = exe_dir
        .join("dist")
        .join("node_modules")
        .join("node-gyp")
        .join("bin");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(entry_dir.join("node-gyp.js"), "#!/usr/bin/env node\n").unwrap();
}

fn entry_path(exe_dir: &Path) -> std::path::PathBuf {
    exe_dir
        .join("dist")
        .join("node_modules")
        .join("node-gyp")
        .join("bin")
        .join("node-gyp.js")
}

#[test]
fn finds_the_entry_point_in_the_wrappers_payload() {
    let exe_dir = tempfile::tempdir().unwrap();
    ship_payload(exe_dir.path());
    ship_entry(exe_dir.path());

    let bin_dir = bundled_node_gyp_bin_in(exe_dir.path()).unwrap();

    assert_eq!(node_gyp_entry_in_payload(&bin_dir), Some(entry_path(exe_dir.path())));
}

/// Without the `node-gyp.js` the wrapper delegates to, a script given
/// that path would fail, so the variable stays unset.
#[test]
fn entry_absent_when_only_the_wrapper_was_shipped() {
    let exe_dir = tempfile::tempdir().unwrap();
    ship_payload(exe_dir.path());

    let bin_dir = bundled_node_gyp_bin_in(exe_dir.path()).unwrap();

    assert_eq!(node_gyp_entry_in_payload(&bin_dir), None);
}

/// An entry beside the launch symlink is not part of the payload the
/// wrapper came from, so it is never picked over the target's own entry.
#[cfg(unix)]
#[test]
fn entry_comes_from_the_same_payload_as_the_wrapper() {
    use super::bundled_node_gyp_bin_beside;

    let package_dir = tempfile::tempdir().unwrap();
    ship_payload(package_dir.path());
    ship_entry(package_dir.path());
    let exe = package_dir.path().join("pnpm");
    fs::write(&exe, "").unwrap();
    let link_dir = tempfile::tempdir().unwrap();
    ship_entry(link_dir.path());
    let link = link_dir.path().join("pnpm");
    std::os::unix::fs::symlink(&exe, &link).unwrap();

    let bin_dir = bundled_node_gyp_bin_beside(&link).unwrap();

    assert_eq!(
        node_gyp_entry_in_payload(&bin_dir),
        Some(entry_path(&dunce::canonicalize(package_dir.path()).unwrap())),
    );
}
