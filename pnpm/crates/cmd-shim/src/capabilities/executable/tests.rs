use super::{FsSetPermissions, ensure_executable_bits};
use std::{
    fs::{self, Permissions},
    io,
    os::unix::fs::PermissionsExt,
    path::Path,
};
use tempfile::tempdir;

#[test]
fn executable_owned_by_another_user_does_not_require_chmod() {
    struct DeniedPermissions;
    impl FsSetPermissions for DeniedPermissions {
        fn set_permissions(_: &Path, _: Permissions) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("node_modules/foo/cli.js");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "#!/usr/bin/env node\n").unwrap();
    for mode in [0o555, 0o755] {
        fs::set_permissions(&target, Permissions::from_mode(mode)).unwrap();
        ensure_executable_bits::<DeniedPermissions>(&target, None).unwrap();
    }
}

#[test]
fn target_missing_executable_bits_has_bits_added() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("node_modules/foo/cli.js");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "#!/usr/bin/env node\n").unwrap();
    for initial_mode in [0o644, 0o744] {
        fs::set_permissions(&target, Permissions::from_mode(initial_mode)).unwrap();
        ensure_executable_bits::<crate::capabilities::Host>(&target, None).unwrap();
        let mode = fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, initial_mode | 0o111);
    }
}

#[test]
fn target_in_the_installed_modules_dir_has_bits_added() {
    let tmp = tempdir().unwrap();
    let modules_dir = tmp.path().join("vendor");
    let installed = modules_dir.join("foo/cli.js");
    let elsewhere = tmp.path().join("tools/foo/cli.js");
    for target in [&installed, &elsewhere] {
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, "#!/usr/bin/env node\n").unwrap();
        fs::set_permissions(target, Permissions::from_mode(0o644)).unwrap();
        ensure_executable_bits::<crate::capabilities::Host>(target, Some(&modules_dir)).unwrap();
    }
    let mode = |path: &Path| {
        fs::metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&installed), 0o755);
    assert_eq!(mode(&elsewhere), 0o644);
}
