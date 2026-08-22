use super::{write_atomic, write_new_atomic};
use std::io;
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

#[test]
fn new_atomic_write_does_not_replace_an_occupied_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bin/node");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "another command").unwrap();

    let error = write_new_atomic(&path, b"pnpm shim", Some(0o755)).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "another command");
}

#[cfg(unix)]
#[test]
fn new_atomic_write_publishes_the_requested_mode() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bin/node");

    write_new_atomic(&path, b"pnpm shim", Some(0o755)).unwrap();

    let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
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
