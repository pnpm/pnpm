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
        ensure_executable_bits::<DeniedPermissions>(&target).unwrap();
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
        ensure_executable_bits::<crate::capabilities::Host>(&target).unwrap();
        let mode = fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, initial_mode | 0o111);
    }
}
