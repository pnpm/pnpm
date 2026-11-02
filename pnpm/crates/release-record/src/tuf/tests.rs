use super::{
    TrustedRepository, TrustedVersions, TufError, canonical,
    metadata::{Root, Snapshot, Targets, Timestamp, parse},
    role::verify_role,
};
use crate::test_support::{FUTURE, PAST, RoleKeys, key, root, sign, snapshot, targets, timestamp};
use chrono::{DateTime, Utc};
use ed25519_dalek::SigningKey;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::{Digest as _, Sha256};

fn now() -> DateTime<Utc> {
    "2026-11-01T00:00:00Z".parse().unwrap()
}

const POLICY: &[u8] = b"{\"schema\":\"pnpr-log-policy/1\"}";

/// Metadata for one repository state: a root, then timestamp, snapshot, and
/// targets that list [`POLICY`].
struct Fixture {
    root_keys: Vec<SigningKey>,
    timestamp_key: SigningKey,
    snapshot_key: SigningKey,
    targets_key: SigningKey,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            root_keys: vec![key(1), key(2)],
            timestamp_key: key(3),
            snapshot_key: key(4),
            targets_key: key(5),
        }
    }

    fn role_keys(&self) -> RoleKeys<'_> {
        RoleKeys {
            root: self.root_keys.iter().collect(),
            root_threshold: 2,
            timestamp: &self.timestamp_key,
            snapshot: &self.snapshot_key,
            targets: &self.targets_key,
        }
    }

    fn root(&self, version: u64, expires: &str) -> Vec<u8> {
        let signers: Vec<&SigningKey> = self.root_keys.iter().collect();
        sign(&root(version, expires, &self.role_keys()), &signers)
    }

    /// Timestamp, snapshot, and targets bytes at `version`.
    fn top_level(&self, version: u64, expires: &str) -> [Vec<u8>; 3] {
        let targets = sign(
            &targets(version, expires, &[("pnpr-log-policy.json", POLICY)]),
            &[&self.targets_key],
        );
        let snapshot = sign(&snapshot(version, expires, version, &targets), &[&self.snapshot_key]);
        let timestamp =
            sign(&timestamp(version, expires, version, &snapshot), &[&self.timestamp_key]);
        [timestamp, snapshot, targets]
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn load(
    repository: &mut TrustedRepository,
    [timestamp, snapshot, targets]: &[Vec<u8>; 3],
) -> Result<(), TufError> {
    repository.update_timestamp(timestamp, now())?;
    repository.update_snapshot(snapshot, now())?;
    repository.update_targets(targets, now())
}

#[test]
fn verifies_a_target_through_the_whole_chain() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    load(&mut repository, &fixture.top_level(1, FUTURE)).unwrap();
    repository.verify_target("pnpr-log-policy.json", POLICY).unwrap();
    assert!(matches!(
        repository.verify_target("pnpr-log-policy.json", b"{\"schema\":\"forged\"}"),
        Err(TufError::FileMismatch { .. }),
    ));
    assert!(matches!(
        repository.verify_target("other.json", POLICY),
        Err(TufError::MissingTarget { .. })
    ));
    assert_eq!(
        repository.versions(),
        TrustedVersions { timestamp: Some(1), snapshot: Some(1), targets: Some(1) },
    );
}

#[test]
fn rejects_a_first_root_that_is_not_pinned() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    assert!(matches!(
        TrustedRepository::new(&first_root, [0; 32]),
        Err(TufError::RootDigestMismatch)
    ));
    let second_root = fixture.root(2, FUTURE);
    assert!(matches!(
        TrustedRepository::new(&second_root, digest(&second_root)),
        Err(TufError::RootVersion { expected: 1, actual: 2 }),
    ));
}

#[test]
fn rotates_root_keys_only_with_both_thresholds() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let rotated = Fixture { root_keys: vec![key(11), key(12)], ..Fixture::new() };
    let next = root(2, FUTURE, &rotated.role_keys());

    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    let old_only = sign(&next, &[&fixture.root_keys[0], &fixture.root_keys[1]]);
    assert!(matches!(
        repository.update_root(&old_only),
        Err(TufError::Threshold { role: "root", .. })
    ));
    let new_only = sign(&next, &[&rotated.root_keys[0], &rotated.root_keys[1]]);
    assert!(matches!(
        repository.update_root(&new_only),
        Err(TufError::Threshold { role: "root", .. })
    ));
    let one_old =
        sign(&next, &[&fixture.root_keys[0], &rotated.root_keys[0], &rotated.root_keys[1]]);
    assert!(matches!(
        repository.update_root(&one_old),
        Err(TufError::Threshold { role: "root", .. })
    ));

    let both = sign(
        &next,
        &[
            &fixture.root_keys[0],
            &fixture.root_keys[1],
            &rotated.root_keys[0],
            &rotated.root_keys[1],
        ],
    );
    repository.update_root(&both).unwrap();
    assert_eq!(repository.root_version(), 2);
    assert_eq!(repository.root_bytes(), both);

    let skipped = rotated.root(4, FUTURE);
    assert!(matches!(
        repository.update_root(&skipped),
        Err(TufError::RootVersion { expected: 3, actual: 4 })
    ));
}

#[test]
fn a_fresh_client_follows_a_timestamp_key_rotation_and_refuses_the_old_key() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let rotated = Fixture { timestamp_key: key(13), ..Fixture::new() };
    let second_root = rotated.root(2, FUTURE);

    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    repository.update_root(&second_root).unwrap();
    let [stale_timestamp, ..] = fixture.top_level(1, FUTURE);
    assert!(matches!(
        repository.update_timestamp(&stale_timestamp, now()),
        Err(TufError::Threshold { role: "timestamp", found: 0, required: 1 }),
    ));
    load(&mut repository, &rotated.top_level(1, FUTURE)).unwrap();
    repository.verify_target("pnpr-log-policy.json", POLICY).unwrap();
}

#[test]
fn rotating_the_timestamp_key_clears_trusted_versions() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let versions = TrustedVersions { timestamp: Some(9), snapshot: Some(9), targets: Some(9) };
    let mut repository =
        TrustedRepository::resume(digest(&first_root), &first_root, versions).unwrap();
    let rotated = Fixture { timestamp_key: key(13), ..Fixture::new() };
    repository
        .update_root(&rotated.root(2, FUTURE))
        .unwrap();
    assert_eq!(repository.versions(), TrustedVersions::default());
    load(&mut repository, &rotated.top_level(1, FUTURE)).unwrap();
}

#[test]
fn rejects_expired_metadata() {
    let fixture = Fixture::new();
    let expired_root = fixture.root(1, PAST);
    let mut repository = TrustedRepository::new(&expired_root, digest(&expired_root)).unwrap();
    let [timestamp, ..] = fixture.top_level(1, FUTURE);
    assert!(matches!(
        repository.update_timestamp(&timestamp, now()),
        Err(TufError::Expired { role: "root" })
    ));

    let first_root = fixture.root(1, FUTURE);
    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    let [timestamp, ..] = fixture.top_level(1, PAST);
    assert!(matches!(
        repository.update_timestamp(&timestamp, now()),
        Err(TufError::Expired { role: "timestamp" })
    ));

    let [_, snapshot, targets] = fixture.top_level(1, PAST);
    let fresh = fixture.top_level(1, FUTURE);
    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    repository
        .update_timestamp(&fresh[0], now())
        .unwrap();
    assert!(
        repository
            .update_snapshot(&snapshot, now())
            .is_err(),
    );
    repository
        .update_snapshot(&fresh[1], now())
        .unwrap();
    assert!(
        repository
            .update_targets(&targets, now())
            .is_err(),
    );
}

#[test]
fn rejects_rollbacks_against_persisted_versions() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let resume =
        |versions| TrustedRepository::resume(digest(&first_root), &first_root, versions).unwrap();
    let [timestamp, snapshot, _] = fixture.top_level(3, FUTURE);

    let mut repository =
        resume(TrustedVersions { timestamp: Some(4), ..TrustedVersions::default() });
    assert!(matches!(
        repository.update_timestamp(&timestamp, now()),
        Err(TufError::Rollback { role: "timestamp", trusted: 4, offered: 3 }),
    ));
    let mut repository =
        resume(TrustedVersions { snapshot: Some(4), ..TrustedVersions::default() });
    assert!(matches!(
        repository.update_timestamp(&timestamp, now()),
        Err(TufError::Rollback { role: "snapshot", trusted: 4, offered: 3 }),
    ));
    let mut repository = resume(TrustedVersions { targets: Some(4), ..TrustedVersions::default() });
    repository
        .update_timestamp(&timestamp, now())
        .unwrap();
    assert!(matches!(
        repository.update_snapshot(&snapshot, now()),
        Err(TufError::Rollback { role: "targets", trusted: 4, offered: 3 }),
    ));
    let mut repository =
        resume(TrustedVersions { timestamp: Some(3), snapshot: Some(3), targets: Some(3) });
    load(&mut repository, &fixture.top_level(3, FUTURE)).unwrap();
}

#[test]
fn rejects_metadata_that_does_not_match_its_listing() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let [timestamp, _, targets] = fixture.top_level(2, FUTURE);
    let other_snapshot = sign(&snapshot(2, FUTURE, 3, &targets), &[&fixture.snapshot_key]);
    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    repository
        .update_timestamp(&timestamp, now())
        .unwrap();
    assert!(matches!(
        repository.update_snapshot(&other_snapshot, now()),
        Err(TufError::FileMismatch { .. })
    ));

    let unhashed_listing =
        sign(&timestamp_without_hashes(&other_snapshot), &[&fixture.timestamp_key]);
    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    repository
        .update_timestamp(&unhashed_listing, now())
        .unwrap();
    repository
        .update_snapshot(&other_snapshot, now())
        .unwrap();
    assert!(matches!(
        repository.update_targets(&targets, now()),
        Err(TufError::FileMismatch { .. } | TufError::VersionMismatch { .. }),
    ));
}

fn timestamp_without_hashes(snapshot_bytes: &[u8]) -> serde_json::Value {
    let mut value = timestamp(2, FUTURE, 2, snapshot_bytes);
    value["meta"]["snapshot.json"] = json!({ "version": 2 });
    value
}

#[test]
fn requires_roles_in_order() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    let [_, snapshot, targets] = fixture.top_level(1, FUTURE);
    assert!(matches!(
        repository.update_snapshot(&snapshot, now()),
        Err(TufError::NotLoaded { role: "timestamp" })
    ));
    assert!(matches!(
        repository.update_targets(&targets, now()),
        Err(TufError::NotLoaded { role: "snapshot" })
    ));
    assert!(matches!(
        repository.verify_target("x", b""),
        Err(TufError::NotLoaded { role: "targets" })
    ));
}

#[test]
fn rejects_metadata_of_the_wrong_role() {
    let fixture = Fixture::new();
    let first_root = fixture.root(1, FUTURE);
    let mut repository = TrustedRepository::new(&first_root, digest(&first_root)).unwrap();
    let [_, snapshot, _] = fixture.top_level(1, FUTURE);
    assert!(matches!(
        repository.update_timestamp(&snapshot, now()),
        Err(TufError::WrongType { expected: "timestamp", .. })
    ));
}

#[test]
fn encodes_canonical_json_as_securesystemslib_does() {
    let value = json!({ "b": [1, -2, true, null], "a": "line\nquote\"back\\slash", "ä": {} });
    assert_eq!(
        String::from_utf8(canonical::encode(&value).unwrap()).unwrap(),
        "{\"a\":\"line\nquote\\\"back\\\\slash\",\"b\":[1,-2,true,null],\"ä\":{}}",
    );
    assert!(matches!(canonical::encode(&json!({ "x": 1.5 })), Err(TufError::NonIntegerNumber)));
}

/// Metadata generated by `python-tuf`, the TUF reference implementation,
/// vendored from the `tough` test data (MIT OR Apache-2.0): its timestamp,
/// snapshot, and targets roles are signed with Ed25519 keys over
/// `securesystemslib` canonical JSON.
#[test]
fn verifies_python_tuf_role_signatures() {
    let fixture = |name: &str| {
        std::fs::read(format!(
            "{}/tests/fixtures/tuf-reference-impl/{name}",
            env!("CARGO_MANIFEST_DIR"),
        ))
        .unwrap()
    };
    let root = parse::<Root>(&fixture("1.root.json")).unwrap();
    verify_role(&root.body, "timestamp", &parse::<Timestamp>(&fixture("timestamp.json")).unwrap())
        .unwrap();
    verify_role(&root.body, "snapshot", &parse::<Snapshot>(&fixture("snapshot.json")).unwrap())
        .unwrap();
    verify_role(&root.body, "targets", &parse::<Targets>(&fixture("targets.json")).unwrap())
        .unwrap();
    let timestamp_as_targets = parse::<Timestamp>(&fixture("timestamp.json")).unwrap();
    assert!(verify_role(&root.body, "targets", &timestamp_as_targets).is_err());

    // Its root is signed with an RSA key, which this client does not verify,
    // so bootstrapping from it fails closed.
    let first_root = fixture("1.root.json");
    assert!(matches!(
        TrustedRepository::new(&first_root, digest(&first_root)),
        Err(TufError::Threshold { role: "root", found: 0, required: 1 }),
    ));
}
