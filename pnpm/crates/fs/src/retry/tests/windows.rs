use crate::{remove_dir_all_with_retry, remove_file_with_retry, rename_with_retry};
use std::{
    fs, io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    process::Command,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use tempfile::tempdir;
use windows_sys::Win32::{
    Security::{
        AdjustTokenPrivileges, ImpersonateSelf, RevertToSelf, SecurityImpersonation,
        TOKEN_ADJUST_PRIVILEGES,
    },
    System::Threading::{GetCurrentThread, OpenThreadToken},
};

fn assert_fails_promptly(operation: impl FnOnce() -> io::Result<()>) {
    let started = Instant::now();
    let error = operation().expect_err("the protected entry must not be changed");
    let elapsed = started.elapsed();
    assert_eq!(error.raw_os_error(), Some(5));
    eprintln!("access denied after {elapsed:?}");
    assert!(elapsed < Duration::from_secs(5));
}

#[test]
fn readonly_destination_rename_fails_promptly() {
    let root = tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    let protected = tree.join("file");
    let source = root.path().join("source");
    fs::write(&protected, "preserved").unwrap();
    fs::write(&source, "replacement").unwrap();
    let original = fs::metadata(&protected).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&protected, readonly).unwrap();

    assert_fails_promptly(|| rename_with_retry(&source, &protected));

    fs::set_permissions(&protected, original).unwrap();
    assert_eq!(fs::read_to_string(protected).unwrap(), "preserved");
    assert_eq!(fs::read_to_string(source).unwrap(), "replacement");
}

fn icacls(path: &Path, arguments: &[&str]) {
    let output = Command::new("icacls").arg(path).args(arguments).output().unwrap();
    eprintln!("icacls {path:?} {arguments:?}: {output:?}");
    assert!(output.status.success(), "icacls failed: {output:?}");
}

#[test]
fn restrictive_acls_fail_promptly() {
    let root = tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    let protected = tree.join("file");
    let destination = root.path().join("destination");
    fs::write(&protected, "preserved").unwrap();
    icacls(&tree, &["/inheritance:r", "/grant:r", "*S-1-1-0:(RX,WDAC)"]);
    icacls(&protected, &["/inheritance:r", "/grant:r", "*S-1-1-0:(R,WDAC)"]);
    icacls(&tree, &[]);
    icacls(&protected, &[]);
    without_thread_privileges(|| {
        assert_eq!(fs::remove_file(&protected).unwrap_err().raw_os_error(), Some(5));
        assert_fails_promptly(|| remove_file_with_retry(&protected));
        assert_fails_promptly(|| rename_with_retry(&protected, &destination));
        assert_fails_promptly(|| remove_dir_all_with_retry(&tree));
    });

    icacls(&tree, &["/reset"]);
    icacls(&protected, &["/reset"]);
    assert_eq!(fs::read_to_string(protected).unwrap(), "preserved");
    assert!(!destination.exists(), "failed rename must not create the destination");
}

#[test]
fn directory_destinations_fail_promptly() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::write(&source, "source").unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("child"), "preserved").unwrap();

    assert_fails_promptly(|| rename_with_retry(&source, &destination));
    assert_fails_promptly(|| remove_file_with_retry(&destination));

    assert_eq!(fs::read_to_string(source).unwrap(), "source");
    assert_eq!(fs::read_to_string(destination.join("child")).unwrap(), "preserved");
}

#[test]
fn directory_rename_recovers_after_a_child_handle_closes() {
    use std::os::windows::fs::OpenOptionsExt as _;

    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    let child = source.join("child");
    fs::write(&child, "preserved").unwrap();
    let handle = fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).open(&child).unwrap();
    assert_eq!(fs::rename(&source, &destination).unwrap_err().raw_os_error(), Some(5));

    std::thread::scope(|scope| {
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(handle);
        });
        rename_with_retry(&source, &destination).unwrap();
    });

    assert_eq!(fs::read_to_string(destination.join("child")).unwrap(), "preserved");
}

// Elevated test runners can bypass ACLs through backup/restore privileges.
// Impersonation confines the privilege change to this test's thread.
fn without_thread_privileges(operation: impl FnOnce()) {
    struct RevertImpersonation;
    impl Drop for RevertImpersonation {
        fn drop(&mut self) {
            // SAFETY: this guard stays on the thread that called ImpersonateSelf.
            assert_ne!(unsafe { RevertToSelf() }, 0, "{}", io::Error::last_os_error());
        }
    }

    assert_ne!(
        // SAFETY: SecurityImpersonation is a valid level; no pointers are passed.
        unsafe { ImpersonateSelf(SecurityImpersonation) },
        0,
        "{}",
        io::Error::last_os_error()
    );
    let _revert = RevertImpersonation;
    let mut token = null_mut();
    assert_ne!(
        // SAFETY: GetCurrentThread returns a valid pseudo-handle and token is a writable output.
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_ADJUST_PRIVILEGES, 1, &raw mut token) },
        0,
        "{}",
        io::Error::last_os_error()
    );
    // SAFETY: OpenThreadToken succeeded and ownership of its handle transfers exactly once.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    assert_ne!(
        // SAFETY: the token has TOKEN_ADJUST_PRIVILEGES access. Disabling all privileges
        // permits null state/output pointers and a zero buffer length.
        unsafe {
            AdjustTokenPrivileges(token.as_raw_handle(), 1, null(), 0, null_mut(), null_mut())
        },
        0,
        "{}",
        io::Error::last_os_error()
    );
    operation();
}
