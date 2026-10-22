//! The content digests and records [`super::super::validation`] keeps.

use super::super::{
    pnpmfiles_drift,
    validation::{
        FileDigest, FileDigests, RecordValidation, Vouched, manifest_digest, record_validation,
    },
};
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::{FileSignature, Validation, WorkspaceState};
use std::{
    collections::HashSet,
    fs,
    path::Path,
    time::{Duration, SystemTime},
};
use tempfile::tempdir;

fn manifest(json: &str) -> PackageManifest {
    PackageManifest::from_value(
        "/project/package.json".into(),
        serde_json::from_str(json).expect("manifest json"),
    )
}

#[test]
fn a_manifest_digest_ignores_key_order_and_layout() {
    let digest = manifest_digest(&manifest(r#"{"name":"a","dependencies":{"x":"1","y":"2"}}"#));
    assert_eq!(
        manifest_digest(&manifest(
            "{\n  \"dependencies\": { \"y\": \"2\", \"x\": \"1\" },\n  \"name\": \"a\"\n}",
        )),
        digest,
    );
    assert_ne!(
        manifest_digest(&manifest(r#"{"name":"a","dependencies":{"x":"1","y":"3"}}"#)),
        digest,
    );
}

/// Strings are length-prefixed, so moving a character from a key into its
/// value changes the digest.
#[test]
fn a_manifest_digest_separates_keys_from_values() {
    assert_ne!(
        manifest_digest(&manifest(r#"{"a":"bc"}"#)),
        manifest_digest(&manifest(r#"{"ab":"c"}"#)),
    );
}

fn validation_recording(path: &Path, signature: FileSignature, recorded_at_ns: i64) -> Validation {
    Validation {
        recorded_at_ns,
        files: [(path.to_string_lossy().into_owned(), signature)].into(),
        ..Validation::default()
    }
}

fn signature_of(path: &Path) -> FileSignature {
    match FileDigests::new(None).of(path) {
        FileDigest::Present(signature) => signature,
        other => panic!("expected a digest of {path:?}, got {other:?}"),
    }
}

/// A recorded stat the file still has stands for its content, so the file
/// is not read again.
#[test]
fn a_matching_stat_reuses_the_recorded_digest() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("pnpm-lock.yaml");
    fs::write(&path, "lockfileVersion: '9.0'\n").unwrap();
    let signature = FileSignature { digest: "recorded".into(), ..signature_of(&path) };
    let recorded_later = signature.mtime_ns.max(signature.ctime_ns.unwrap_or(0)) + 1_000_000_000;
    let validation = validation_recording(&path, signature.clone(), recorded_later);
    assert_eq!(FileDigests::new(Some(&validation)).of(&path), FileDigest::Present(signature));
}

/// A file written in the clock tick its stat was recorded in may have been
/// written again without its stat moving, so its content is read again.
#[test]
fn a_stat_recorded_in_the_same_tick_is_not_trusted() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("pnpm-lock.yaml");
    fs::write(&path, "lockfileVersion: '9.0'\n").unwrap();
    let actual = signature_of(&path);
    let stale = FileSignature { digest: "recorded".into(), ..actual };
    let same_tick = stale.mtime_ns.max(stale.ctime_ns.unwrap_or(0));
    let validation = validation_recording(&path, stale, same_tick);
    assert_eq!(FileDigests::new(Some(&validation)).of(&path), FileDigest::Present(actual));
}

#[test]
fn a_project_proven_against_another_lockfile_keeps_that_lockfile() {
    let mut validation = Validation { lockfile: Some("current".into()), ..Validation::default() };
    validation.insert_project("a".into(), "manifest-a".into(), Some("current"));
    validation.insert_project("b".into(), "manifest-b".into(), Some("earlier"));
    assert_eq!(validation.project("a").unwrap().lockfile, Some("current"));
    assert_eq!(validation.project("b").unwrap().lockfile, Some("earlier"));
    assert!(!validation.lockfiles.contains_key("a"), "a record that matches is not repeated");
    assert!(validation.project("c").is_none());
}

/// A filtered write vouches for the projects it installed and carries the
/// records of the others, unless an input every project shares changed.
#[test]
fn a_filtered_write_carries_the_other_records_while_the_shared_inputs_stand() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let mut config = Config::new();
    config.modules_dir = root.join("node_modules");
    config.install_state_dir = config.modules_dir.join(".pnpm");
    fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let [a, b] = ["a", "b"].map(|name| manifest(&format!(r#"{{"name":"{name}"}}"#)));
    let project_manifests = [(root.join("a"), &a), (root.join("b"), &b)];
    let record = |state: &mut WorkspaceState, vouched: &Vouched<'_>| {
        record_validation(
            state,
            &RecordValidation {
                workspace_root: root,
                config: &config,
                project_manifests: &project_manifests,
                vouched,
            },
        );
    };
    let mut previous = WorkspaceState::default();
    record(&mut previous, &Vouched::All);

    let only_a = HashSet::from([root.join("a")]);
    let mut filtered = WorkspaceState::default();
    record(&mut filtered, &Vouched::Projects { dirs: &only_a, previous: Some(&previous) });
    let manifests = &filtered.validation.as_ref().unwrap().manifests;
    assert_eq!(manifests.keys().collect::<Vec<_>>(), ["a", "b"]);
    assert!(!filtered.filtered_install);

    let mut drifted = WorkspaceState::default();
    drifted.settings.dedupe_peers = Some(true);
    record(&mut drifted, &Vouched::Projects { dirs: &only_a, previous: Some(&previous) });
    let manifests = &drifted.validation.as_ref().unwrap().manifests;
    assert_eq!(manifests.keys().collect::<Vec<_>>(), ["a"]);
    assert!(drifted.filtered_install, "a project left unproven makes the state filtered");
}

/// pnpm v11 hashes manifests the same way, so either CLI recognizes what the
/// other recorded. The TypeScript test of `manifestDigest` pins the same value.
#[test]
fn a_manifest_digest_matches_the_one_pnpm_v11_computes() {
    let manifest = manifest(
        r#"{"name":"a","version":"1.0.0","dependencies":{"@scope/x":"^1.0.0","y":"2"},"peerDependenciesMeta":{"y":{"optional":true}},"files":["lib"],"weird":[1,2.5,null,false,"é"]}"#,
    );
    assert_eq!(manifest_digest(&manifest), "0684f5a6122e59859b6c170262a4c110");
}

#[test]
fn source_date_epoch_records_deterministic_pnpmfile_digests_without_trusting_mtimes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let pnpmfile = root.join(".pnpmfile.cjs");
    fs::write(&pnpmfile, "module.exports = {}\n").unwrap();
    fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    pnpm_testing_utils::fs::set_mtime(&pnpmfile, epoch + Duration::from_hours(1));

    let mut config = Config::new();
    config.modules_dir = root.join("node_modules");
    config.install_state_dir = config.modules_dir.join(".pnpm");
    config.source_date_epoch = Some(epoch);
    let root_manifest = manifest(r#"{"name":"a"}"#);
    let project_manifests = [(root.to_path_buf(), &root_manifest)];
    let mut state = WorkspaceState {
        last_validated_timestamp: 1_700_000_000_000,
        pnpmfiles: vec![pnpmfile.to_string_lossy().into_owned()],
        ..WorkspaceState::default()
    };
    record_validation(
        &mut state,
        &RecordValidation {
            workspace_root: root,
            config: &config,
            project_manifests: &project_manifests,
            vouched: &Vouched::All,
        },
    );

    let validation = state.validation.as_ref().expect("validation recorded");
    assert_eq!(validation.recorded_at_ns, 0);
    for signature in validation.files.values() {
        assert_eq!(signature.mtime_ns, 0);
        assert_eq!(signature.ctime_ns, None);
    }
    assert_eq!(
        pnpmfiles_drift(root, &config, &state),
        None,
        "an unchanged pnpmfile newer than SOURCE_DATE_EPOCH must not be flagged as modified",
    );

    fs::write(&pnpmfile, "module.exports = { hooks: {} }\n").unwrap();
    pnpm_testing_utils::fs::set_mtime(&pnpmfile, epoch - Duration::from_hours(1));
    assert!(
        pnpmfiles_drift(root, &config, &state).is_some(),
        "an edited pnpmfile with mtime restored before SOURCE_DATE_EPOCH must be detected",
    );
}
