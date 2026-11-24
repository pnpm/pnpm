use super::mirror_is_older_than;
use jiff::SignedDuration;
use std::{
    fs,
    time::{
        Duration,
        SystemTime,
    },
};

const FIVE_MINUTES: SignedDuration = SignedDuration::from_mins(5);

#[test]
fn a_missing_mirror_is_older_than_any_age() {
    let dir = tempfile::tempdir().expect("create a temp dir");
    assert!(mirror_is_older_than(Some(&dir.path().join("missing.json")), FIVE_MINUTES));
    assert!(mirror_is_older_than(None, FIVE_MINUTES));
}

#[test]
fn a_mirror_is_older_than_the_age_it_was_written_before() {
    let dir = tempfile::tempdir().expect("create a temp dir");
    let mirror = dir.path().join("pkg.json");
    fs::write(&mirror, "{}").expect("write the mirror");
    assert!(!mirror_is_older_than(Some(&mirror), FIVE_MINUTES));

    let long_ago = SystemTime::now() - Duration::from_mins(10);
    fs::File::options()
        .write(true)
        .open(&mirror)
        .and_then(|file| file.set_modified(long_ago))
        .expect("age the mirror");
    assert!(mirror_is_older_than(Some(&mirror), FIVE_MINUTES));
}
