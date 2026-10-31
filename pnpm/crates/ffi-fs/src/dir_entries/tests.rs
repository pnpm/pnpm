use super::directory_entry_names;
use std::{ffi::OsStr, fs};

fn sorted(directory: &fs::File) -> Vec<String> {
    let mut names: Vec<String> = directory_entry_names(directory)
        .expect("read the directory")
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `.` and `..` are entries `readdir` reports and no caller wants, and
/// `open_directory_at` refuses them, so they are dropped here.
#[test]
fn the_dot_entries_are_left_out() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::write(fixture.path().join("file"), b"x").expect("write the file");
    fs::create_dir(fixture.path().join("inside")).expect("create inside");
    let directory = fs::File::open(fixture.path()).expect("open the fixture");

    assert_eq!(sorted(&directory), ["file", "inside"]);
}

#[test]
fn an_empty_directory_reads_as_no_names() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    let directory = fs::File::open(fixture.path()).expect("open the fixture");

    assert!(directory_entry_names(&directory).expect("read it").is_empty());
}

/// The handle is the caller's, and `closedir` releases the stream's own
/// descriptor, so reading twice through the same handle has to work.
#[test]
fn the_handle_survives_the_read() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::write(fixture.path().join("file"), b"x").expect("write the file");
    let directory = fs::File::open(fixture.path()).expect("open the fixture");

    assert_eq!(sorted(&directory), ["file"]);
    assert_eq!(sorted(&directory), ["file"], "the second read sees the same names");
}

fn assert_every_listing_is_whole(directory: &fs::File) {
    for _ in 0..200 {
        assert_eq!(sorted(directory), ["file", "inside"]);
    }
}

/// A duplicate of the handle would share its read position, so listings
/// that overlap would consume each other's entries. The stream reads a
/// descriptor opened through the handle instead.
#[test]
fn a_listing_is_whole_when_listings_overlap() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::write(fixture.path().join("file"), b"x").expect("write the file");
    fs::create_dir(fixture.path().join("inside")).expect("create inside");
    let directory = fs::File::open(fixture.path()).expect("open the fixture");

    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| assert_every_listing_is_whole(&directory));
        }
    });
}

/// A name `readdir` reports is one component, which is what
/// `open_directory_at` and `unlink_at` accept.
#[test]
fn every_name_is_one_component() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::write(fixture.path().join("a.b"), b"x").expect("write the file");
    let directory = fs::File::open(fixture.path()).expect("open the fixture");

    for name in directory_entry_names(&directory).expect("read it") {
        assert!(!name.as_encoded_bytes().contains(&b'/'), "{name:?}");
        assert_ne!(name, OsStr::new("."));
        assert_ne!(name, OsStr::new(".."));
    }
}
