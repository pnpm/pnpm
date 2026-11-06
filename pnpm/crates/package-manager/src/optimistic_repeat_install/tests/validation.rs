//! The content digests and records [`super::super::validation`] keeps.

use super::super::{
    pnpmfiles_drift,
    validation::{
        FileDigest, FileDigests, RecordValidation, Vouched, manifest_digest, manifest_stand_ins,
        project_bins_match, record_validation,
    },
};
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::ProjectManifestPaths;
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
                project_manifests: crate::DeclaredManifests::new(&project_manifests),
                workspace_projects: None,
                vouched,
                lockfile: None,
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
            project_manifests: crate::DeclaredManifests::new(&project_manifests),
            workspace_projects: None,
            vouched: &Vouched::All,
            lockfile: None,
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

#[test]
fn workspace_bin_digests_match_between_implementations_and_ignore_layout() {
    use super::super::validation::workspace_bins_digest;
    use std::path::PathBuf;

    let root = Path::new("/project");
    let package = manifest(r#"{"name":"a","bin":{"a-one":"one.js","a-two":"two.js"}}"#);
    let projects = [(root.join("packages/a"), &package)];
    let digest = workspace_bins_digest(root, &projects);
    assert_eq!(digest.as_deref(), Some("341026c69559cca8573860896362ff03"));
    let reordered = manifest(
        r#"{"bin":{"a-two":"two.js","a-one":"one.js"},"name":"a","description":"changed"}"#,
    );
    assert_eq!(workspace_bins_digest(root, &[(root.join("packages/a"), &reordered)]), digest);
    let moved = PathBuf::from("/moved");
    assert_eq!(workspace_bins_digest(&moved, &[(moved.join("packages/a"), &package)]), digest);
    for changed in [
        r#"{"name":"a","bin":{"a-one":"two.js","a-two":"two.js"}}"#,
        r#"{"name":"a","bin":{"a-renamed":"one.js","a-two":"two.js"}}"#,
        r#"{"name":"a","bin":{"a-one":"one.js"}}"#,
        r#"{"name":"a","bin":"one.js"}"#,
        r#"{"name":"renamed","bin":{"a-one":"one.js","a-two":"two.js"}}"#,
        r#"{"name":"a","directories":{"bin":"bin"}}"#,
        r#"{"name":"a"}"#,
    ] {
        let changed = manifest(changed);
        assert_ne!(workspace_bins_digest(root, &[(root.join("packages/a"), &changed)]), digest);
    }
    for value in [r#"{"name":"a"}"#, r#"{"bin":null,"directories":{"bin":null}}"#] {
        let package = manifest(value);
        assert_eq!(workspace_bins_digest(root, &[(root.join("packages/a"), &package)]), None);
    }
}

#[test]
fn a_state_without_a_validation_record_matches_any_project_bins() {
    assert!(project_bins_match(None, ".", Some("341026c69559cca8573860896362ff03")));
    let recorded = Validation::default();
    assert!(!project_bins_match(Some(&recorded), ".", Some("341026c69559cca8573860896362ff03")));
    assert!(project_bins_match(Some(&recorded), "packages/a", None));
}

/// A workspace whose projects' manifests are on disk, with every file dated
/// an hour back and the filesystem clock past their ctimes, so a later
/// recording trusts their stats.
fn project_files(root: &Path, manifests: &[(&str, &str)]) -> Vec<PackageManifest> {
    let an_hour_ago = SystemTime::now() - Duration::from_hours(1);
    fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let files = manifests
        .iter()
        .map(|(dir, json)| {
            let path = root.join(dir).join("package.json");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, json).unwrap();
            pnpm_testing_utils::fs::set_mtime(&path, an_hour_ago);
            PackageManifest::from_path(path).unwrap()
        })
        .collect();
    super::wait_for_the_next_filesystem_tick(root);
    files
}

fn record_files(
    root: &Path,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    vouched: &Vouched<'_>,
) -> Validation {
    record_files_with(root, project_manifests, vouched, Config::new())
}

fn record_files_with(
    root: &Path,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    vouched: &Vouched<'_>,
    mut config: Config,
) -> Validation {
    config.modules_dir = root.join("node_modules");
    config.install_state_dir = config.modules_dir.join(".pnpm");
    let mut state = WorkspaceState::default();
    record_validation(
        &mut state,
        &RecordValidation {
            workspace_root: root,
            config: &config,
            project_manifests: crate::DeclaredManifests::new(project_manifests),
            workspace_projects: None,
            vouched,
            lockfile: None,
        },
    );
    state.validation.unwrap()
}

/// Move `validation`'s recording past the rounding window of whole-second
/// filesystems, so its manifest stats are trusted.
fn trust_recorded_stats(validation: &mut Validation) {
    validation.recorded_at_ns = validation.manifest_files
        .values()
        .map(|file| file.mtime_ns.max(file.ctime_ns.unwrap_or(0)))
        .max()
        .unwrap()
        + 3_000_000_000;
}

#[test]
fn a_write_records_each_vouched_manifest_file_with_the_fields_the_check_reads() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let json = r#"{"name":"a","scripts":{"test":"x"},"dependencies":{"b":"workspace:*","c":"git+https://user:token@example.com/c.git","d":"file:///opt/d","e":"https:user:secret@example.com/e.tgz","f":"link:///opt/f","g":"npm:h@1","i":"C:/i","j":" https://user:t@example.com/j.tgz","n":"https://TOKEN@example.com/n.tgz","l":"user:token@github.com:x/l","m":"git@github.com:x/m"},"peerDependencies":{"e":"https://user:secret@example.com/e.tgz","f":"^1"},"bin":"cli.js","publishConfig":{"registry":"https://user:secret@example.com/","directory":"dist"},"peerDependenciesMeta":{"e":{"optional":false,"note":"x"}},"devDependencies":{},"engines":{"node":">=20","pnpm":">=12"},"os":["linux"],"cpu":["x64"],"libc":["glibc"]}"#;
    let project_a = project_files(root, &[("a", json)]).remove(0);
    let validation = record_files(root, &[(root.join("a"), &project_a)], &Vouched::All);
    let file = &validation.manifest_files["a"];
    assert_eq!(file.file_name, "package.json");
    assert_eq!(file.size, json.len() as u64);
    assert_eq!(
        serde_json::Value::Object(file.fields.clone().unwrap()),
        serde_json::json!({
            "name": "a",
            "dependencies": {
                "b": "workspace:*",
                "c": "*",
                "d": "file:///opt/d",
                "e": "*",
                "f": "link:///opt/f",
                "g": "npm:h@1",
                "i": "C:/i",
                "j": "*",
                "n": "*",
                "l": "*",
                "m": "git@github.com:x/m",
            },
            "devDependencies": {},
            "peerDependencies": { "e": "*", "f": "^1" },
            "peerDependenciesMeta": { "e": { "optional": false } },
            "bin": "cli.js",
            "publishConfig": { "directory": "dist" },
            "engines": { "node": ">=20", "pnpm": ">=12" },
            "os": ["linux"],
            "cpu": ["x64"],
            "libc": ["glibc"],
        }),
    );
}

/// A credential in a specifier whose scheme the check reads cannot be
/// replaced without changing the answer, so the record keeps no fields and
/// the next check reads the file.
#[test]
fn a_credential_in_a_checked_specifier_leaves_the_record_without_fields() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    for spec in [
        "npm:b@^1 || https://user:token@example.com/b.tgz",
        "file://TOKEN@example.com/b.tgz",
        "file://us%3Aer@example.com/b.tgz",
        "C:/vendor/@scope/lib",
        "../../vendor/pkg:branch@v1",
        "user@host:token@x",
    ] {
        let json = serde_json::json!({ "name": "a", "dependencies": { "b": spec } }).to_string();
        let project_a = project_files(root, &[("a", &json)]).remove(0);
        let validation = record_files(root, &[(root.join("a"), &project_a)], &Vouched::All);
        assert_eq!(validation.manifest_files["a"].fields, None, "{spec}");
    }
}

#[test]
fn a_manifest_edited_after_it_was_read_is_not_vouched_for_by_its_stat() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let project_a = project_files(root, &[("a", r#"{"name":"a"}"#)]).remove(0);
    fs::write(root.join("a/package.json"), r#"{"name":"a","version":"2.0.0"}"#).unwrap();
    let validation = record_files(root, &[(root.join("a"), &project_a)], &Vouched::All);
    assert!(validation.manifests.contains_key("a"));
    assert!(!validation.manifest_files.contains_key("a"));
}

/// A write takes an unchanged manifest's record from the previous state, so
/// the facts it carries are the recorded ones, not re-derived from the file.
#[test]
fn a_manifest_whose_trusted_stat_is_unchanged_keeps_its_record() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let manifests = project_files(root, &[("a", r#"{"name":"a"}"#), ("b", r#"{"name":"b"}"#)]);
    let project_manifests = [(root.join("a"), &manifests[0]), (root.join("b"), &manifests[1])];
    let mut previous_validation = record_files(root, &project_manifests, &Vouched::All);
    for file in previous_validation.manifest_files.values_mut() {
        file.fields
            .as_mut()
            .unwrap()
            .insert("version".to_owned(), "9.9.9".into());
    }
    trust_recorded_stats(&mut previous_validation);
    let mut without_fields = previous_validation.clone();
    for file in without_fields.manifest_files.values_mut() {
        file.fields = None;
    }
    let rerecorded = record_files(
        root,
        &project_manifests,
        &Vouched::Projects {
            dirs: &HashSet::from([root.join("a"), root.join("b")]),
            previous: Some(&WorkspaceState {
                validation: Some(without_fields),
                ..WorkspaceState::default()
            }),
        },
    );
    assert_eq!(
        rerecorded.manifest_files
            .iter()
            .map(|(importer_id, file)| (importer_id.as_str(), file.fields.is_some()))
            .collect::<Vec<_>>(),
        [("a", true), ("b", true)],
        "a record without fields is not reused",
    );
    let previous =
        WorkspaceState { validation: Some(previous_validation), ..WorkspaceState::default() };
    let only_a = HashSet::from([root.join("a")]);
    let validation = record_files(
        root,
        &project_manifests,
        &Vouched::Projects { dirs: &only_a, previous: Some(&previous) },
    );
    let kept = validation.manifest_files
        .values()
        .map(|file| {
            file.fields
                .as_ref()
                .unwrap()
                .contains_key("version")
        })
        .collect::<Vec<_>>();
    assert_eq!(kept, [true, true], "a reused record for a and a carried one for b");
}

#[test]
fn source_date_epoch_neither_records_nor_carries_manifest_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let manifests = project_files(root, &[("a", r#"{"name":"a"}"#), ("b", r#"{"name":"b"}"#)]);
    let project_manifests = [(root.join("a"), &manifests[0]), (root.join("b"), &manifests[1])];
    let previous_validation = record_files(root, &project_manifests, &Vouched::All);
    assert_eq!(previous_validation.manifest_files.len(), 2);
    let previous =
        WorkspaceState { validation: Some(previous_validation), ..WorkspaceState::default() };
    let only_a = HashSet::from([root.join("a")]);
    let mut config = Config::new();
    config.source_date_epoch = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    let validation = record_files_with(
        root,
        &project_manifests,
        &Vouched::Projects { dirs: &only_a, previous: Some(&previous) },
        config,
    );
    assert!(validation.manifests.contains_key("b"), "b's manifest digest is carried");
    assert!(validation.manifest_files.is_empty());
}

/// A carried record keeps the trust it had when it was recorded, so a later
/// write does not vouch for an edit that kept the untrusted stat.
#[test]
fn a_carried_manifest_file_needs_a_stat_its_own_recording_trusted() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let manifests = project_files(root, &[("a", r#"{"name":"a"}"#), ("b", r#"{"name":"b"}"#)]);
    let project_manifests = [(root.join("a"), &manifests[0]), (root.join("b"), &manifests[1])];
    let mut previous_validation = record_files(root, &project_manifests, &Vouched::All);
    let file_b = previous_validation.manifest_files.get_mut("b").unwrap();
    file_b.mtime_ns = previous_validation.recorded_at_ns;
    file_b.ctime_ns = None;
    let previous =
        WorkspaceState { validation: Some(previous_validation), ..WorkspaceState::default() };
    let only_a = HashSet::from([root.join("a")]);
    let validation = record_files(
        root,
        &project_manifests,
        &Vouched::Projects { dirs: &only_a, previous: Some(&previous) },
    );
    assert!(validation.manifests.contains_key("b"));
    assert!(validation.manifest_files.contains_key("a"));
    assert!(!validation.manifest_files.contains_key("b"));
}

/// `dir`'s manifest candidates as a `<dir>/*` listing holds them.
fn listed(root: &Path, dir: &str) -> ProjectManifestPaths {
    let root_dir = root.join(dir);
    let candidates = ["package.json", "package.json5", "package.yaml"]
        .map(|basename| root_dir.join(basename))
        .to_vec();
    ProjectManifestPaths { root_dir, candidates }
}

#[test]
fn records_stand_in_for_the_manifests_whose_stat_still_holds() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let manifests = project_files(
        root,
        &[("a", r#"{"name":"a","scripts":{"x":"y"},"dependencies":{"b":"workspace:*"}}"#)],
    );
    let mut validation = record_files(root, &[(root.join("a"), &manifests[0])], &Vouched::All);
    trust_recorded_stats(&mut validation);
    fs::create_dir_all(root.join("no-project")).unwrap();

    let stand_ins =
        manifest_stand_ins(root, &validation, &[listed(root, "a"), listed(root, "no-project")])
            .expect("the record stands in");

    let [project] = stand_ins.projects.as_slice() else { panic!("one project") };
    assert_eq!(project.root_dir, root.join("a"));
    assert_eq!(project.manifest.path(), root.join("a/package.json"));
    assert_eq!(
        project.manifest.value(),
        &serde_json::json!({ "name": "a", "dependencies": { "b": "workspace:*" } }),
    );
    assert_eq!(stand_ins.digests[&root.join("a")], validation.manifests["a"]);
}

#[test]
fn records_cannot_stand_in_once_a_manifest_may_have_changed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let manifests = project_files(root, &[("a", r#"{"name":"a"}"#), ("b", r#"{"name":"b"}"#)]);
    let project_manifests = [(root.join("a"), &manifests[0]), (root.join("b"), &manifests[1])];
    let mut validation = record_files(root, &project_manifests, &Vouched::All);
    trust_recorded_stats(&mut validation);
    let stand_ins = |validation: &Validation| {
        manifest_stand_ins(root, validation, &[listed(root, "a"), listed(root, "b")]).is_some()
    };
    assert!(stand_ins(&validation));

    let mut without_fields = validation.clone();
    without_fields.manifest_files.get_mut("b").unwrap().fields = None;
    assert!(!stand_ins(&without_fields), "a record without fields");

    let mut older_revision = validation.clone();
    older_revision.manifest_files.get_mut("b").unwrap().fields_revision = 0;
    assert!(!stand_ins(&older_revision), "a record of another field revision");

    let mut unrecorded = validation.clone();
    unrecorded.manifest_files.remove("b");
    assert!(!stand_ins(&unrecorded), "a project without a record");

    let mut untrusted = validation.clone();
    untrusted.recorded_at_ns = 0;
    assert!(!stand_ins(&untrusted), "a stat recorded before the file changed");

    fs::write(root.join("b/package.yaml"), "name: b\n").unwrap();
    fs::remove_file(root.join("b/package.json")).unwrap();
    assert!(!stand_ins(&validation), "another manifest file");
    fs::write(root.join("b/package.json"), r#"{"name":"b"}"#).unwrap();
    assert!(!stand_ins(&validation), "a rewritten manifest");
}

/// An edit that keeps the size and restores the mtime still moves the change
/// time.
#[test]
fn records_cannot_stand_in_for_a_same_size_edit_with_its_mtime_restored() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let manifests = project_files(root, &[("a", r#"{"name":"a","dependencies":{"b":"1"}}"#)]);
    let mut validation = record_files(root, &[(root.join("a"), &manifests[0])], &Vouched::All);
    trust_recorded_stats(&mut validation);
    assert!(manifest_stand_ins(root, &validation, &[listed(root, "a")]).is_some());
    let path = root.join("a/package.json");
    let mtime = fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap();
    super::wait_for_the_next_filesystem_tick(root);

    fs::write(&path, r#"{"name":"a","dependencies":{"b":"2"}}"#).unwrap();
    pnpm_testing_utils::fs::set_mtime(&path, mtime);

    assert!(manifest_stand_ins(root, &validation, &[listed(root, "a")]).is_none());
}
