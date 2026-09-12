use super::write_atomic;
use tempfile::TempDir;

#[test]
fn writes_content_and_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested/auth.ini");
    write_atomic(&path, b"//host/:_authToken=tok\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "//host/:_authToken=tok\n");
}

#[test]
fn replaces_existing_content() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("auth.ini");
    write_atomic(&path, b"old").unwrap();
    write_atomic(&path, b"new").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
}

#[cfg(unix)]
#[test]
fn preserves_existing_file_mode() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("auth.ini");
    std::fs::write(&path, "old").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    write_atomic(&path, b"new").unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "existing mode must be preserved, got {mode:o}");
}

#[cfg(unix)]
#[test]
fn does_not_follow_a_symlinked_target() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = TempDir::new().unwrap();
    // A symlinked credential file pointing at a permissive (0644) file: the
    // write must replace the link with a fresh 0600 regular file, leaving the
    // link target untouched, rather than overwriting through the link.
    let real = dir.path().join("real.ini");
    std::fs::write(&real, "secret").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
    let link = dir.path().join("auth.ini");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    write_atomic(&link, b"new").unwrap();

    assert!(!std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "new");
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "secret", "link target untouched");
    let mode = std::fs::metadata(&link).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "a replaced symlink keeps the conservative default, got {mode:o}");
}

/// A credential must not inherit a world-readable mode from the settings file
/// it is being written into.
#[cfg(unix)]
#[test]
fn write_atomic_private_does_not_inherit_a_readable_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, "nodeLinker: hoisted\n").expect("seed the file");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("widen the mode");

    super::write_atomic_private(&path, b"_auth: {}\n").expect("write the credential");

    let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "got {mode:o}");
}

#[test]
fn concurrent_replacements_leave_complete_content() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("state.json");
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        for byte in b'a'..=b'd' {
            let path = &path;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                replace_repeatedly(path, byte);
            });
        }
    });
    let contents = std::fs::read(path).unwrap();
    assert_eq!(contents.len(), 4096);
    assert!(contents.iter().all(|byte| *byte == contents[0]));
}

fn replace_repeatedly(path: &std::path::Path, byte: u8) {
    for _ in 0..20 {
        write_atomic(path, &[byte; 4096]).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn waits_for_a_locked_destination_before_replacing_it() {
    use std::os::windows::fs::OpenOptionsExt as _;

    let directory = TempDir::new().unwrap();
    let path = directory.path().join("state.json");
    std::fs::write(&path, "old").unwrap();
    let locked = std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(locked);
        });
        write_atomic(&path, b"new").unwrap();
    });
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
}
