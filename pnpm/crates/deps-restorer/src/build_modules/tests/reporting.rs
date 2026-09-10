use super::super::slots::parse_name_version_from_key;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

#[test]
fn parse_scoped_key() {
    let (name, version) = parse_name_version_from_key("/@pnpm.e2e/install-script-example@1.0.0");
    assert_eq!(name, "@pnpm.e2e/install-script-example");
    assert_eq!(version, "1.0.0");
}
#[test]
fn parse_unscoped_key() {
    let (name, version) = parse_name_version_from_key("/is-positive@1.0.0");
    assert_eq!(name, "is-positive");
    assert_eq!(version, "1.0.0");
}
/// Scoped package: `<lockfile>/node_modules/@scope/pkg`. The
/// `@`-scope guard never fires when paths are absolute (the dirname's
/// leading char is `/`), so every step is pushed regardless of segment
/// shape, including the scope slot's synthetic
/// `<scope>/node_modules/.bin`. See the helper's doc-comment for the
/// rationale.
#[test]
fn bin_dirs_scoped_pkg_pushes_every_step() {
    let lockfile_dir = PathBuf::from("/repo");
    let pkg_root = PathBuf::from("/repo/node_modules/@scope/pkg");
    let dirs = super::super::bin_dirs_in_all_parent_dirs(&pkg_root, &lockfile_dir);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/repo/node_modules/@scope/pkg/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/@scope/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/node_modules/.bin"),
            PathBuf::from("/repo/node_modules/.bin"),
        ],
    );
}
