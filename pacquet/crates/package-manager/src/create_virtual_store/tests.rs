use super::{
    CreateVirtualStoreError, InstallPackageBySnapshotError, emit_warm_snapshot_progress,
    integrity_equal, snapshot_cache_key, snapshot_deps_equal,
};
use pacquet_lockfile::{
    GitResolution, LockfileResolution, PackageKey, PackageMetadata, PkgName, PkgVerPeer,
    RegistryResolution, SnapshotDepRef, SnapshotEntry, TarballResolution,
};
use pacquet_reporter::{LogEvent, ProgressMessage, Reporter};
use std::{collections::HashMap, sync::Mutex};

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn metadata_with_integrity(integrity: &str) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: integrity.parse().expect("parse integrity"),
        }),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn snapshot_with_dep(child: &str, ref_str: &str) -> SnapshotEntry {
    let dep_ref: SnapshotDepRef = ref_str.parse().expect("parse SnapshotDepRef");
    SnapshotEntry {
        dependencies: Some(HashMap::from([(name(child), dep_ref)])),
        ..Default::default()
    }
}

/// `emit_warm_snapshot_progress` fires `resolved` then, when the
/// package was *not* network-fetched this install, `found_in_store` in
/// that order for one (package_id, requester) pair. Both events carry
/// the same identifiers — pnpm's per-package counter relies on the pair
/// to pin the tick to the right package row.
#[test]
fn emits_resolved_then_found_in_store_when_not_network_fetched() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_warm_snapshot_progress::<RecordingReporter>("react@18.0.0", "/proj", false);

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [
                LogEvent::Progress(r),
                LogEvent::Progress(f),
            ] if matches!(
                &r.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj"
            ) && matches!(
                &f.message,
                ProgressMessage::FoundInStore { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj",
            ),
        ),
        "warm-snapshot pair must be (Resolved, FoundInStore) with matching identifiers; got {captured:?}",
    );
}

/// When the package *was* network-fetched earlier this install (the
/// fresh path's silent resolve-time prefetcher pulled it), the second
/// event is `fetched`, not `found_in_store` — so a cold `pacquet
/// install` reports its prefetch downloads as downloads, matching
/// `--frozen-lockfile`. Regression guard for
/// <https://github.com/pnpm/pnpm/issues/12235>.
#[test]
fn emits_resolved_then_fetched_when_network_fetched() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_warm_snapshot_progress::<RecordingReporter>("react@18.0.0", "/proj", true);

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [
                LogEvent::Progress(r),
                LogEvent::Progress(f),
            ] if matches!(
                &r.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj"
            ) && matches!(
                &f.message,
                ProgressMessage::Fetched { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj",
            ),
        ),
        "network-fetched warm snapshot must report (Resolved, Fetched); got {captured:?}",
    );
}

/// `snapshot_deps_equal` is `true` when both `dependencies` and
/// `optionalDependencies` agree — matching upstream's `equals(...)`
/// pair. An absent map matches an empty map: pnpm canonicalises both
/// to `{}` via Ramda's `isEmpty`, so pacquet must too or warm
/// reinstalls would loop pointlessly when the lockfile drops the
/// optional-deps key.
#[test]
fn snapshot_deps_equal_treats_absent_and_empty_alike() {
    let absent = SnapshotEntry::default();
    let empty = SnapshotEntry {
        dependencies: Some(HashMap::new()),
        optional_dependencies: Some(HashMap::new()),
        ..Default::default()
    };
    assert!(snapshot_deps_equal(&absent, &empty));
    assert!(snapshot_deps_equal(&empty, &absent));
}

/// A real diff on `dependencies` flips the result to `false`. Upstream
/// gates the skip on this comparison; if pacquet treated mismatched
/// child-version edges as "no change", a warm reinstall would silently
/// keep an outdated symlink layout when the lockfile bumped a
/// transitive.
#[test]
fn snapshot_deps_equal_distinguishes_different_dependency_values() {
    let entry_a = snapshot_with_dep("react", "17.0.2");
    let entry_b = snapshot_with_dep("react", "18.0.0");
    assert!(!snapshot_deps_equal(&entry_a, &entry_b));
}

/// `optionalDependencies` participate in the comparison the same way
/// `dependencies` do — both upstream `equals` calls have to agree
/// before the skip fires.
#[test]
fn snapshot_deps_equal_distinguishes_different_optional_dependency_values() {
    let dep_ref: SnapshotDepRef = "1.0.0".parse().expect("parse dep ref");
    let entry_a = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(name("react"), dep_ref.clone())])),
        ..Default::default()
    };
    let entry_b = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(name("react-dom"), dep_ref)])),
        ..Default::default()
    };
    assert!(!snapshot_deps_equal(&entry_a, &entry_b));
}

/// `integrity_equal` mirrors upstream's `isIntegrityEqual` —
/// identical `integrity` strings on both sides means the cached
/// tarball is still valid, mismatched (or one-sided) integrities
/// force a re-fetch.
#[test]
fn integrity_equal_matches_when_integrities_agree() {
    let entry_a = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let entry_b = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    assert!(integrity_equal(Some(&entry_a), Some(&entry_b)));
}

#[test]
fn integrity_equal_distinguishes_changed_integrities() {
    let entry_a = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let entry_b = metadata_with_integrity(
        "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
    );
    assert!(!integrity_equal(Some(&entry_a), Some(&entry_b)));
}

/// Missing metadata on either side (a malformed lockfile, or the
/// snapshot referring to a `packages:` entry that was dropped)
/// collapses to `None` on the integrity lookup. Both sides `None`
/// stays "equal" so a directory/git resolution pair (whose integrity
/// is `None`) doesn't trip a spurious re-fetch.
#[test]
fn integrity_equal_treats_none_pair_as_equal() {
    assert!(integrity_equal(None, None));
}

#[test]
fn integrity_equal_treats_one_sided_missing_as_unequal() {
    let with_integrity = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    assert!(!integrity_equal(None, Some(&with_integrity)));
    assert!(!integrity_equal(Some(&with_integrity), None));
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

fn git_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Git(GitResolution {
            repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
            commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
            path: None,
        }),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn git_hosted_tarball_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
            integrity: None,
            git_hosted: Some(true),
            path: None,
        }),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

/// `Git` resolutions go through the warm batch under a
/// `gitHostedStoreIndexKey`-shaped key (`pkg_id\tbuilt|not-built`),
/// not under the integrity-based key. This is the read-side mirror
/// of what both fetchers write at install time — a drift between
/// the two would silently degrade every git-hosted re-install to
/// the cold path.
#[test]
fn snapshot_cache_key_for_git_resolution_uses_git_hosted_key() {
    let pkg = key("ts-pipe-compose", "0.2.1");
    let packages = HashMap::from([(pkg.clone(), git_metadata())]);

    let received = snapshot_cache_key(&pkg, &packages).expect("snapshot_cache_key must not error");
    assert_eq!(
        received,
        Some(format!("{pkg}\tbuilt")),
        "git resolutions must route through gitHostedStoreIndexKey",
    );
}

/// `Tarball { gitHosted: true }` mirrors the bare-`Git` arm — same
/// key shape, so the warm prefetch picks up both fetchers' rows
/// the same way.
#[test]
fn snapshot_cache_key_for_git_hosted_tarball_uses_git_hosted_key() {
    let pkg = key("foo", "1.0.0");
    let packages = HashMap::from([(pkg.clone(), git_hosted_tarball_metadata())]);

    let received = snapshot_cache_key(&pkg, &packages).expect("snapshot_cache_key must not error");
    assert_eq!(
        received,
        Some(format!("{pkg}\tbuilt")),
        "git-hosted tarball resolutions must route through gitHostedStoreIndexKey",
    );
}

/// Failing closed at the cache-key site (rather than only at the
/// install-side guard) is the whole point of the check duplication —
/// otherwise a malformed lockfile burns the warm rayon batch before
/// the install path fires the same error.
#[test]
fn snapshot_cache_key_rejects_tarball_without_integrity() {
    let pkg = key("foo", "1.0.0");
    let packages = HashMap::from([(pkg.clone(), tarball_metadata_without_integrity())]);

    let err =
        snapshot_cache_key(&pkg, &packages).expect_err("missing integrity must reject upfront");
    assert!(
        matches!(
            &err,
            CreateVirtualStoreError::InstallPackageBySnapshot(
                InstallPackageBySnapshotError::MissingTarballIntegrity { package_key },
            ) if package_key == &pkg.to_string(),
        ),
        "expected MissingTarballIntegrity for `{pkg}`, got {err:?}",
    );
}

fn tarball_metadata_without_integrity() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz".to_string(),
            integrity: None,
            git_hosted: None,
            path: None,
        }),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}
