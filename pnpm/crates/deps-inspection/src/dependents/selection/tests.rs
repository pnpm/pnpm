use std::path::Path;

use pnpm_lockfile::Lockfile;

use super::package_merge_id;

#[test]
fn package_merge_id_resolves_directories_lexically_and_preserves_unc_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        r"lockfileVersion: '9.0'

importers:
  .: {}

packages:
  foo@file:../../vendor/foo-v1:
    resolution: {type: directory, directory: ../../vendor/foo-v1}

  foo@file:../../vendor/foo-v1/:
    resolution: {type: directory, directory: ../../vendor/foo-v1/}

  foo@file:../../../vendor/foo-v1:
    resolution: {type: directory, directory: ../../../vendor/foo-v1}

  foo@file:../../vendor/foo-v2:
    resolution: {type: directory, directory: ../../vendor/foo-v2}

  unc-bs@file:share:
    resolution: {type: directory, directory: '\\server\share\foo'}

  unc-slash@file:share:
    resolution: {type: directory, directory: '//server/share/foo/'}

snapshots:
  foo@file:../../vendor/foo-v1: {}
  foo@file:../../vendor/foo-v1/: {}
  foo@file:../../../vendor/foo-v1: {}
  foo@file:../../vendor/foo-v2: {}
  unc-bs@file:share: {}
  unc-slash@file:share: {}
",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();

    let p1 = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"foo@file:../../vendor/foo-v1".parse().unwrap(),
        "",
    );
    let p1_trailing = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"foo@file:../../vendor/foo-v1/".parse().unwrap(),
        "",
    );
    let p2 = package_merge_id(
        Path::new("/workspace/packages/nested/project-2"),
        &lockfile,
        &"foo@file:../../../vendor/foo-v1".parse().unwrap(),
        "",
    );
    let p3 = package_merge_id(
        Path::new("/workspace/packages/project-3"),
        &lockfile,
        &"foo@file:../../vendor/foo-v2".parse().unwrap(),
        "",
    );
    assert_eq!(p1, p1_trailing);
    assert_eq!(p1, p2);
    assert_ne!(p1, p3);

    let unc_bs = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"unc-bs@file:share".parse().unwrap(),
        "",
    );
    let unc_slash = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"unc-slash@file:share".parse().unwrap(),
        "",
    );
    assert_eq!(unc_bs.as_deref(), Some("dir://server/share/foo"));
    assert_eq!(unc_bs, unc_slash);
}
