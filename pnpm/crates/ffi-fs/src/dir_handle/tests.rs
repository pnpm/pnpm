use super::{
    DirectoryAccess, create_directory_at, create_file_at, open_directory_at,
    open_directory_for_traversal, open_file_at, open_file_at_without_blocking, read_link_at,
    rename_at, symlink_at, unlink_at,
};
use std::{ffi::OsStr, fs, io, os::unix::fs::FileTypeExt as _, path::Path};

fn open_root(path: &Path) -> fs::File {
    fs::File::open(path).expect("open the fixture root")
}

/// Pins that the directory a handle names is the one entries are resolved
/// in, which is the property the whole module exists for.
#[test]
fn an_entry_resolves_in_the_handles_directory() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir(fixture.path().join("inside")).expect("create inside");
    fs::write(fixture.path().join("inside/file"), b"content").expect("write file");
    let root = open_root(fixture.path());

    let inside = open_directory_at(&root, OsStr::new("inside")).expect("open inside");
    let file = open_file_at(&inside, OsStr::new("file")).expect("open the file");
    assert_eq!(io::read_to_string(file).expect("read the file"), "content");
}

/// A symlink at the name is refused, so a replaced component cannot
/// redirect the lookup after the handle was opened.
#[test]
fn a_symlink_at_the_name_is_refused() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir(fixture.path().join("real")).expect("create real");
    std::os::unix::fs::symlink("real", fixture.path().join("link")).expect("create the symlink");
    let root = open_root(fixture.path());

    // `O_NOFOLLOW` with `O_DIRECTORY` is reported as `ENOTDIR` on Linux and
    // as `ELOOP` on macOS, so callers branch on both and so does this.
    let error = open_directory_at(&root, OsStr::new("link")).expect_err("the symlink is refused");
    assert!(
        error.kind() == io::ErrorKind::NotADirectory || error.raw_os_error() == Some(libc::ELOOP),
        "{error:?}",
    );
}

#[test]
fn a_created_directory_is_opened_by_the_handle_that_created_it() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    let root = open_root(fixture.path());

    create_directory_at(&root, OsStr::new("made"), 0o755).expect("create made");
    let made = open_directory_at(&root, OsStr::new("made")).expect("open made");
    assert!(
        made.metadata()
            .expect("stat made")
            .is_dir(),
    );
}

#[test]
fn creating_a_file_twice_reports_the_name_as_taken() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    let root = open_root(fixture.path());

    create_file_at(&root, OsStr::new("once"), 0o600).expect("create once");
    let error = create_file_at(&root, OsStr::new("once"), 0o600).expect_err("the name is taken");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error:?}");
}

/// Pins the non-blocking open on the entry that makes the difference: a
/// FIFO with no writer, whose blocking open would never return.
#[test]
fn a_fifo_opens_without_waiting_for_a_writer() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    crate::create_fifo(&fixture.path().join("pipe"), 0o600).expect("create the FIFO");
    let root = open_root(fixture.path());

    let pipe = open_file_at_without_blocking(&root, OsStr::new("pipe")).expect("open the FIFO");
    assert!(
        pipe.metadata()
            .expect("stat the FIFO")
            .file_type()
            .is_fifo(),
    );
}

#[test]
fn a_renamed_entry_leaves_its_old_name() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::write(fixture.path().join("before"), b"content").expect("write before");
    let root = open_root(fixture.path());

    rename_at(&root, OsStr::new("before"), OsStr::new("after")).expect("rename");
    assert!(!fixture.path().join("before").exists());
    assert_eq!(fs::read_to_string(fixture.path().join("after")).expect("read after"), "content");
}

#[test]
fn an_unlinked_entry_is_gone() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::write(fixture.path().join("doomed"), b"").expect("write doomed");
    let root = open_root(fixture.path());

    unlink_at(&root, OsStr::new("doomed")).expect("unlink");
    assert!(!fixture.path().join("doomed").exists());
    let error = unlink_at(&root, OsStr::new("doomed")).expect_err("it is already gone");
    assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error:?}");
}

#[test]
fn a_symlink_reads_back_the_target_it_was_given() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    let root = open_root(fixture.path());

    symlink_at(Path::new("../elsewhere"), &root, OsStr::new("link")).expect("create the symlink");
    assert_eq!(
        read_link_at(&root, OsStr::new("link")).expect("read the symlink"),
        Path::new("../elsewhere"),
    );
}

/// A target longer than the first buffer is read whole, which is the branch
/// the doubling loop exists for.
#[test]
fn a_long_symlink_target_reads_back_whole() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    let root = open_root(fixture.path());
    let target: String = std::iter::repeat_n("abcdefgh/", 60).collect();

    symlink_at(Path::new(&target), &root, OsStr::new("link")).expect("create the symlink");
    assert_eq!(
        read_link_at(&root, OsStr::new("link")).expect("read the symlink"),
        Path::new(&target),
    );
}

/// Reading a plain file as a symlink is how `readlinkat` reports "not a
/// symlink", and callers branch on exactly this error.
#[test]
fn reading_a_plain_file_as_a_symlink_reports_invalid_input() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::write(fixture.path().join("plain"), b"").expect("write plain");
    let root = open_root(fixture.path());

    let error = read_link_at(&root, OsStr::new("plain")).expect_err("it is not a symlink");
    assert_eq!(error.raw_os_error(), Some(libc::EINVAL), "{error:?}");
}

#[test]
fn a_readable_directory_opens_readable() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir(fixture.path().join("open")).expect("create open");
    let root = open_root(fixture.path());

    let (_handle, access) =
        open_directory_for_traversal(&root, OsStr::new("open")).expect("open for traversal");
    assert_eq!(access, DirectoryAccess::Readable);
}

/// A name holding a NUL cannot reach the filesystem, and is rejected rather
/// than silently truncated at the NUL.
#[test]
fn a_name_holding_a_nul_is_rejected() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    let root = open_root(fixture.path());

    let error = open_file_at(&root, OsStr::new("a\0b")).expect_err("the name is rejected");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error:?}");
}
