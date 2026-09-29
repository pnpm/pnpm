use super::{
    android_user_lock_root, open_secure_lock_file, secure_temp_lock_dir, secure_user_lock_dir,
    xdg_runtime_dir,
};

#[test]
fn android_lock_root_falls_back_when_home_is_unavailable() {
    let fallback = std::path::Path::new("/data/local/tmp");
    assert_eq!(android_user_lock_root(None), fallback);
    let empty_home = Some(std::ffi::OsString::new());
    assert_eq!(android_user_lock_root(empty_home), fallback);
}

#[test]
fn android_lock_root_uses_home_when_available() {
    assert_eq!(
        android_user_lock_root(Some(std::ffi::OsString::from("/data/user/0/pnpm"))),
        std::path::Path::new("/data/user/0/pnpm/.cache"),
    );
}

#[test]
fn xdg_runtime_dir_is_used_only_when_an_owned_absolute_directory() {
    let owned = tempfile::tempdir().unwrap();
    assert_eq!(xdg_runtime_dir(Some(owned.path().into())), Some(owned.path().to_path_buf()));
    assert_eq!(xdg_runtime_dir(None), None);
    assert_eq!(xdg_runtime_dir(Some(std::ffi::OsString::new())), None);
    assert!(std::path::Path::new(".").is_dir());
    assert_eq!(xdg_runtime_dir(Some(".".into())), None);
    assert_eq!(xdg_runtime_dir(Some(owned.path().join("missing").into())), None);
}

#[test]
fn xdg_runtime_dir_that_others_can_write_to_is_ignored() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let set_mode = |mode| {
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    };
    set_mode(0o777);
    assert_eq!(xdg_runtime_dir(Some(root.path().into())), None);
    set_mode(0o770);
    assert_eq!(xdg_runtime_dir(Some(root.path().into())), None);
    set_mode(0o1777);
    assert_eq!(xdg_runtime_dir(Some(root.path().into())), None);
    set_mode(0o755);
    assert_eq!(xdg_runtime_dir(Some(root.path().into())), Some(root.path().to_path_buf()));
}

#[test]
fn xdg_runtime_dir_its_owner_cannot_write_to_is_ignored() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let set_mode = |mode| {
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    };
    set_mode(0o500);
    assert_eq!(xdg_runtime_dir(Some(root.path().into())), None);
    set_mode(0o600);
    assert_eq!(xdg_runtime_dir(Some(root.path().into())), None);
    set_mode(0o700);
    assert_eq!(xdg_runtime_dir(Some(root.path().into())), Some(root.path().to_path_buf()));
}

#[test]
fn xdg_runtime_dir_owned_by_another_user_is_ignored() {
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    assert_eq!(xdg_runtime_dir(Some("/".into())), None);
}

#[cfg(unix)]
#[test]
fn creates_a_private_user_owned_lock_directory() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let name = format!("pnpm-secure-lock-test-{}", std::process::id());
    let directory = secure_temp_lock_dir(&name).unwrap();
    let metadata = std::fs::symlink_metadata(&directory).unwrap();
    assert!(metadata.is_dir());
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    std::fs::remove_dir(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn refuses_a_symlinked_lock_file() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    std::fs::write(&target, "").unwrap();
    let link = root.path().join("lock");
    std::os::unix::fs::symlink(target, &link).unwrap();
    assert!(open_secure_lock_file(&link).is_err());
}

#[cfg(unix)]
#[test]
fn refuses_a_hardlinked_lock_file() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    std::fs::write(&target, "").unwrap();
    let link = root.path().join("lock");
    std::fs::hard_link(target, &link).unwrap();
    assert!(open_secure_lock_file(&link).is_err());
}

#[cfg(not(target_os = "android"))]
#[test]
fn stable_user_lock_directory_does_not_use_the_process_temp_root() {
    const CHILD: &str = "PNPM_STABLE_LOCK_DIRECTORY_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let process_temp = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "secure_temp_lock::tests::stable_user_lock_directory_does_not_use_the_process_temp_root",
            ])
            .env(CHILD, "1")
            .env("TMPDIR", process_temp.path())
            .env_remove("XDG_RUNTIME_DIR")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        return;
    }

    assert_ne!(std::env::temp_dir(), std::path::Path::new("/tmp"));
    let name = format!("pnpm-stable-lock-test-{}", std::process::id());
    let directory = secure_user_lock_dir(&name).unwrap();
    assert_eq!(directory.parent().unwrap(), std::path::Path::new("/tmp"));
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn user_lock_directory_lives_in_the_xdg_runtime_dir() {
    const CHILD: &str = "PNPM_XDG_RUNTIME_LOCK_DIRECTORY_TEST_CHILD";
    let Some(runtime_dir) = std::env::var_os(CHILD) else {
        let runtime_dir = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "secure_temp_lock::tests::user_lock_directory_lives_in_the_xdg_runtime_dir",
            ])
            .env(CHILD, runtime_dir.path())
            .env("XDG_RUNTIME_DIR", runtime_dir.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        return;
    };

    let directory = secure_user_lock_dir("pnpm-xdg-lock-test").unwrap();
    assert_eq!(directory.parent().unwrap(), std::path::Path::new(&runtime_dir));
}
