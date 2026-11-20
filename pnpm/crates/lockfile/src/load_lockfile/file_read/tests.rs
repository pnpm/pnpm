use super::{
    read_lockfile_file,
    stable_open_file_metadata,
};
use std::{
    fs,
    io::Write,
};

#[test]
fn stable_read_keeps_metadata_and_in_place_mutation_drops_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("pnpm-lock.yaml");
    let initial = "lockfileVersion: '9.0'\n";
    fs::write(&path, initial).expect("write initial");

    let (text, meta) = read_lockfile_file(&path).expect("read").expect("present");
    assert_eq!(text, initial);
    assert!(meta.is_some());

    let handle = fs::File::open(&path).expect("open reader");
    let before = handle.metadata().ok();
    assert!(
        stable_open_file_metadata(before.as_ref(), handle.metadata().ok(), initial.len() + 1)
            .is_none(),
        "byte-count mismatch during read must drop metadata",
    );

    let mut writer = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open writer");
    writer.write_all(b"lockfileVersion: \"9.0\"\n").expect("overwrite same length");
    writer.set_modified(std::time::SystemTime::UNIX_EPOCH).expect("touch mtime");
    assert!(
        stable_open_file_metadata(before.as_ref(), handle.metadata().ok(), initial.len()).is_none(),
        "same-size in-place edit between pre/post stat must drop metadata",
    );
}
