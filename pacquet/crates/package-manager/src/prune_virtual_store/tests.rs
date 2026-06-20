use std::{
    collections::HashSet,
    fs,
    time::{Duration, SystemTime},
};

use pacquet_lockfile::PkgNameVerPeer;

use super::{
    prune_target_within_modules, prune_virtual_store, same_dir, should_prune_virtual_store,
};
use crate::SkippedSnapshots;

const SEVEN_DAYS_MINUTES: u64 = 7 * 24 * 60;

/// A fixed reference instant so the throttle tests don't depend on the
/// wall clock.
fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn at(seconds_ago: u64) -> String {
    httpdate::fmt_http_date(now() - Duration::from_secs(seconds_ago))
}

#[test]
fn global_virtual_store_never_prunes() {
    assert!(!should_prune_virtual_store(true, None, SEVEN_DAYS_MINUTES, now()));
    assert!(!should_prune_virtual_store(true, Some(&at(0)), SEVEN_DAYS_MINUTES, now()));
}

#[test]
fn missing_modules_file_prunes() {
    assert!(should_prune_virtual_store(false, None, SEVEN_DAYS_MINUTES, now()));
}

/// Defensive contract only: the real caller never passes `Some("")`
/// because `read_modules_manifest` defaults an empty on-disk `prunedAt`
/// to "now". An empty timestamp is treated as falsy (prune), matching
/// upstream's `modulesFile?.prunedAt` check.
#[test]
fn empty_pruned_at_prunes() {
    assert!(should_prune_virtual_store(false, Some(""), SEVEN_DAYS_MINUTES, now()));
}

#[test]
fn zero_max_age_always_prunes() {
    assert!(should_prune_virtual_store(false, Some(&at(60)), 0, now()));
}

#[test]
fn fresh_cache_is_kept() {
    let one_minute_ago = at(60);
    assert!(!should_prune_virtual_store(false, Some(&one_minute_ago), SEVEN_DAYS_MINUTES, now()));
}

#[test]
fn expired_cache_prunes() {
    let eight_days_ago = at(8 * 24 * 60 * 60);
    assert!(should_prune_virtual_store(false, Some(&eight_days_ago), SEVEN_DAYS_MINUTES, now()));
}

#[test]
fn unparsable_pruned_at_is_kept() {
    assert!(!should_prune_virtual_store(false, Some("not a date"), SEVEN_DAYS_MINUTES, now()));
}

#[test]
fn future_pruned_at_is_kept() {
    let future = httpdate::fmt_http_date(now() + Duration::from_hours(1));
    assert!(!should_prune_virtual_store(false, Some(&future), SEVEN_DAYS_MINUTES, now()));
}

#[test]
fn sweep_keeps_needed_removes_surplus_and_skipped() {
    let max = 120;
    let keep: PkgNameVerPeer = "foo@1.0.0".parse().unwrap();
    let keep_peer: PkgNameVerPeer = "bar@2.0.0(baz@1.0.0)".parse().unwrap();
    let skipped_key: PkgNameVerPeer = "opt@1.0.0".parse().unwrap();

    let store = tempfile::tempdir().unwrap();
    let vsdir = store.path();
    for name in [
        keep.to_virtual_store_name(max),
        keep_peer.to_virtual_store_name(max),
        skipped_key.to_virtual_store_name(max),
        "surplus@9.9.9".to_string(),
        "node_modules".to_string(),
    ] {
        fs::create_dir_all(vsdir.join(&name)).unwrap();
    }
    // The current lockfile lives in the virtual store too; it must be
    // preserved (pacquet's current-lockfile write is conditional, so the
    // sweep deliberately keeps `lock.yaml` rather than orphaning it).
    fs::write(vsdir.join("lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    let keys = [keep.clone(), keep_peer.clone(), skipped_key.clone()];
    let skipped = SkippedSnapshots::from_set(HashSet::from([skipped_key.clone()]));

    let removed = prune_virtual_store(vsdir, keys.iter(), &skipped, max);

    assert_eq!(removed, Some(2));
    assert!(vsdir.join(keep.to_virtual_store_name(max)).exists());
    assert!(vsdir.join(keep_peer.to_virtual_store_name(max)).exists());
    assert!(vsdir.join("node_modules").exists());
    assert!(vsdir.join("lock.yaml").exists());
    assert!(!vsdir.join("surplus@9.9.9").exists());
    assert!(!vsdir.join(skipped_key.to_virtual_store_name(max)).exists());
}

#[test]
fn sweep_on_missing_dir_is_a_noop() {
    let store = tempfile::tempdir().unwrap();
    let vsdir = store.path().join("does-not-exist");
    let keep: PkgNameVerPeer = "foo@1.0.0".parse().unwrap();
    let removed = prune_virtual_store(&vsdir, [keep].iter(), &SkippedSnapshots::new(), 120);
    assert_eq!(removed, Some(0));
}

#[test]
fn sweep_on_unreadable_dir_returns_none() {
    // A path that is a file, not a directory: `read_dir` fails with an
    // error other than `NotFound`, so the sweep reports it didn't run
    // (rather than deleting nothing and looking successful).
    let store = tempfile::tempdir().unwrap();
    let not_a_dir = store.path().join("file");
    fs::write(&not_a_dir, "x").unwrap();
    let keep: PkgNameVerPeer = "foo@1.0.0".parse().unwrap();
    let removed = prune_virtual_store(&not_a_dir, [keep].iter(), &SkippedSnapshots::new(), 120);
    assert_eq!(removed, None);
}

#[test]
fn prune_target_must_be_inside_node_modules() {
    let root = tempfile::tempdir().unwrap();
    let modules = root.path().join("node_modules");
    fs::create_dir_all(&modules).unwrap();

    let inside = modules.join(".pacquet");
    fs::create_dir_all(&inside).unwrap();
    assert_eq!(
        prune_target_within_modules(&inside, &modules),
        Some(fs::canonicalize(&inside).unwrap()),
    );

    assert_eq!(prune_target_within_modules(&modules, &modules), None);

    let outside = root.path().join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    assert_eq!(prune_target_within_modules(&outside, &modules), None);

    let not_created = modules.join("not-created-yet");
    assert_eq!(
        prune_target_within_modules(&not_created, &modules),
        Some(fs::canonicalize(&modules).unwrap().join("not-created-yet")),
    );

    // A not-yet-created path that escapes node_modules is refused even though
    // it is absent at check time: it could be created/swapped in mid-install.
    let outside_missing = root.path().join("not-created-outside");
    assert_eq!(prune_target_within_modules(&outside_missing, &modules), None);
}

#[test]
fn same_dir_matches_equivalent_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    fs::create_dir_all(&store).unwrap();
    assert!(same_dir(&store, &store.join(".")));
    assert!(!same_dir(&store, dir.path()));
}
