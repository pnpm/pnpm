use super::FileMode;
use std::{fs::Permissions, os::unix::fs::PermissionsExt as _};

/// A `stat` mode carries the file's type, and a call that sets a mode
/// wants the permission bits alone. This is the whole reason the type
/// exists, so it is pinned for both of the shapes a reader meets.
#[test]
fn a_file_type_is_dropped() {
    assert_eq!(FileMode::from_mode(0o100644).mode(), 0o644, "a regular file");
    assert_eq!(FileMode::from_mode(0o40755).mode(), 0o755, "a directory");
}

#[test]
fn the_setuid_setgid_and_sticky_bits_are_kept() {
    assert_eq!(FileMode::from_mode(0o7777).mode(), 0o7777);
    assert_eq!(FileMode::from_mode(0o104755).mode(), 0o4755, "setuid survives a file type");
}

/// The path a caller takes when it reads one file's mode and sets
/// another's. The file type goes on the way in and is not put back on the
/// way out.
#[test]
fn a_mode_read_off_a_file_loses_its_file_type() {
    let directory = Permissions::from_mode(0o40755);
    let mode = FileMode::from_permissions(&directory);

    assert_eq!(mode.mode(), 0o755, "the file type is gone");
    assert_eq!(Permissions::from(mode).mode(), 0o755);
}

/// `mode_t` is a `u16` on macOS, so a mode carrying a file type would not
/// fit it. Dropping the type is what makes the conversion infallible.
#[test]
fn every_mode_fits_mode_t() {
    for raw in [0u32, 0o644, 0o777, 0o7777, 0o100644, u32::MAX] {
        // Compared as `u64`, which widens from the `u16` of macOS and the
        // `u32` of Linux alike.
        let mode = FileMode::from_mode(raw);
        assert_eq!(u64::from(mode.narrow()), u64::from(mode.mode()), "{raw:o}");
    }
}
