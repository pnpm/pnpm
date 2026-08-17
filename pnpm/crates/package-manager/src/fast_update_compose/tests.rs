use super::try_compose_fast_updates;
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

/// `foo` and `bar` are prod dependencies (`bar` reaching `child`), and
/// `opt` is an optional dependency reaching the same `child`.
const LOCKFILE: &str = r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
      bar:
        specifier: ^2.0.0
        version: 2.0.0
    optionalDependencies:
      opt:
        specifier: ^5.0.0
        version: 5.0.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-foo
  bar@2.0.0:
    resolution:
      integrity: sha512-bar
  child@3.0.0:
    resolution:
      integrity: sha512-child
  opt@5.0.0:
    resolution:
      integrity: sha512-opt
snapshots:
  foo@1.1.0: {}
  bar@2.0.0:
    dependencies:
      child: 3.0.0
  child@3.0.0: {}
  opt@5.0.0:
    optional: true
    dependencies:
      child: 3.0.0
";

fn lockfile() -> Lockfile {
    serde_saphyr::from_str(LOCKFILE).expect("parse lockfile")
}

fn manifest_from(value: Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("/project/package.json"), value)
}

fn snapshot_keys(lockfile: &Lockfile) -> Vec<String> {
    let mut keys: Vec<_> =
        lockfile.snapshots.as_ref().expect("snapshots").keys().map(ToString::to_string).collect();
    keys.sort();
    keys
}

fn snapshot_optional(lockfile: &Lockfile, key: &str) -> bool {
    lockfile.snapshots.as_ref().expect("snapshots")[&key.parse().expect("snapshot key")].optional
}

#[test]
fn absorbs_a_removal_and_a_widened_ignore_list_in_one_pass() {
    let manifest = manifest_from(json!({
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let config = Config {
        ignored_optional_dependencies: Some(vec!["opt".to_string()]),
        ..Config::default()
    };

    let updated = try_compose_fast_updates(
        &lockfile(),
        &[(".".to_string(), &manifest)],
        &[],
        &config,
        patch_hashes(&config).as_ref(),
        false,
    )
    .expect("both kinds of drift are absorbable at once");

    assert_eq!(
        snapshot_keys(&updated),
        vec!["bar@2.0.0".to_string(), "child@3.0.0".to_string()],
        "the removed dependency and the newly ignored optional both went, with their subtrees",
    );
    assert_eq!(updated.ignored_optional_dependencies, Some(vec!["opt".to_string()]));
    let importer = &updated.importers["."];
    assert!(importer.optional_dependencies.is_none());
    assert!(
        !importer
            .dependencies
            .as_ref()
            .expect("dependencies")
            .contains_key(&"foo".parse().expect("alias")),
    );
}

#[test]
fn absorbs_a_group_move_and_a_settings_change_in_one_pass() {
    let manifest = manifest_from(json!({
        "devDependencies": { "foo": "^1.0.0" },
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let config = Config { auto_install_peers: false, ..Config::default() };
    let mut subject = lockfile();
    subject.settings =
        Some(crate::fast_update_settings::lockfile_settings_from_config(&Config::default()));

    let updated = try_compose_fast_updates(
        &subject,
        &[(".".to_string(), &manifest)],
        &[(PathBuf::from("/project"), &manifest)],
        &config,
        patch_hashes(&config).as_ref(),
        false,
    )
    .expect("a peerless lockfile absorbs the setting alongside the move");

    let importer = &updated.importers["."];
    assert!(
        importer
            .dev_dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&"foo".parse().expect("alias"))),
    );
    assert!(
        !updated.settings.as_ref().expect("settings").auto_install_peers,
        "the settings block rode along with the manifest drift",
    );
}

#[test]
fn falls_back_when_one_of_the_composed_changes_cannot_be_absorbed() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^9.0.0", "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let config = Config { auto_install_peers: false, ..Config::default() };
    let mut subject = lockfile();
    subject.settings =
        Some(crate::fast_update_settings::lockfile_settings_from_config(&Config::default()));

    assert!(
        try_compose_fast_updates(
            &subject,
            &[(".".to_string(), &manifest)],
            &[(PathBuf::from("/project"), &manifest)],
            &config,
            patch_hashes(&config).as_ref(),
            false,
        )
        .is_none(),
        "the incompatible range needs the resolver, and the settings change goes with it",
    );
}

#[test]
fn falls_back_when_a_removal_leaves_a_configured_patch_unused() {
    let dir = workspace(&["bar@2.0.0"]);
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let config = Config { allow_unused_patches: false, ..patch_config(dir.path(), &["bar@2.0.0"]) };

    assert!(
        try_compose_fast_updates(
            &lockfile(),
            &[(".".to_string(), &manifest)],
            &[],
            &config,
            patch_hashes(&config).as_ref(),
            false
        )
        .is_none(),
        "removing the patch's only referent is what the resolver reports as an unused patch",
    );
}

#[test]
fn rekeys_a_patched_survivor_alongside_a_removal() {
    let dir = workspace(&["bar@2.0.0"]);
    let manifest = manifest_from(json!({
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let config = patch_config(dir.path(), &["bar@2.0.0"]);

    let updated = try_compose_fast_updates(
        &lockfile(),
        &[(".".to_string(), &manifest)],
        &[],
        &config,
        patch_hashes(&config).as_ref(),
        false,
    )
    .expect("the removal and the patch rekey compose");

    assert!(
        snapshot_keys(&updated).iter().any(|key| key.starts_with("bar@2.0.0(patch_hash=")),
        "the surviving patched package carries its hash segment",
    );
    assert!(
        !snapshot_keys(&updated).iter().any(|key| key.starts_with("foo@")),
        "while the removed dependency is pruned",
    );
}

/// Records the patch so a later pass sees it already configured, leaving the
/// removal as the only drift.
fn lockfile_recording_a_patch_for_bar(config: &Config) -> Lockfile {
    let keeps_everything = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0", "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    try_compose_fast_updates(
        &lockfile(),
        &[(".".to_string(), &keeps_everything)],
        &[],
        config,
        patch_hashes(config).as_ref(),
        false,
    )
    .expect("the patch rekey alone is absorbed")
}

#[test]
fn falls_back_when_a_removal_orphans_a_patch_the_lockfile_already_records() {
    let dir = workspace(&["bar@2.0.0"]);
    let config = Config { allow_unused_patches: false, ..patch_config(dir.path(), &["bar@2.0.0"]) };
    let subject = lockfile_recording_a_patch_for_bar(&config);
    let drops_bar = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));

    assert!(
        try_compose_fast_updates(
            &subject,
            &[(".".to_string(), &drops_bar)],
            &[],
            &config,
            patch_hashes(&config).as_ref(),
            false
        )
        .is_none(),
        "the patch is left with nothing to apply to, which only a resolution reports",
    );
}

#[test]
fn absorbs_a_removal_that_orphans_a_patch_under_allow_unused_patches() {
    let dir = workspace(&["bar@2.0.0"]);
    let config = patch_config(dir.path(), &["bar@2.0.0"]);
    let subject = lockfile_recording_a_patch_for_bar(&config);
    let drops_bar = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));

    let updated = try_compose_fast_updates(
        &subject,
        &[(".".to_string(), &drops_bar)],
        &[],
        &config,
        patch_hashes(&config).as_ref(),
        false,
    )
    .expect("an unused patch is only a warning here");
    assert!(!snapshot_keys(&updated).iter().any(|key| key.starts_with("bar@")));
}

#[test]
fn falls_back_when_an_ignored_optional_is_embedded_in_a_peer_suffix() {
    let mut subject = lockfile();
    subject.snapshots.as_mut().expect("snapshots").insert(
        "baz@4.0.0(opt@5.0.0)".parse().expect("snapshot key"),
        serde_saphyr::from_str("dependencies:\n  opt: 5.0.0").expect("snapshot"),
    );
    subject
        .importers
        .get_mut(".")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "baz".parse().expect("alias"),
            serde_saphyr::from_str("{specifier: ^4.0.0, version: 4.0.0(opt@5.0.0)}")
                .expect("dependency"),
        );
    let config = Config {
        ignored_optional_dependencies: Some(vec!["opt".to_string()]),
        ..Config::default()
    };

    assert!(
        try_compose_fast_updates(
            &subject,
            &[],
            &[],
            &config,
            patch_hashes(&config).as_ref(),
            false
        )
        .is_none(),
        "the surviving dependent's key embeds the removed package, so it would rekey, not prune",
    );
}

#[test]
fn stands_aside_when_nothing_drifted() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0", "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));

    assert!(
        try_compose_fast_updates(
            &lockfile(),
            &[(".".to_string(), &manifest)],
            &[],
            &Config::default(),
            None,
            false,
        )
        .is_none(),
        "no drift means the ordinary frozen path decides",
    );
}

#[test]
fn recomputes_optional_flags_for_an_ignored_optional_removal() {
    let config = Config {
        ignored_optional_dependencies: Some(vec!["bar".to_string()]),
        ..Config::default()
    };
    let mut subject = lockfile();
    let importer = subject.importers.get_mut(".").expect("importer");
    let moved = importer
        .dependencies
        .as_mut()
        .expect("dependencies")
        .remove(&"bar".parse().expect("alias"))
        .expect("bar");
    importer
        .optional_dependencies
        .as_mut()
        .expect("optionalDependencies")
        .insert("bar".parse().expect("alias"), moved);
    subject
        .snapshots
        .as_mut()
        .expect("snapshots")
        .get_mut(&"bar@2.0.0".parse().expect("key"))
        .expect("snapshot")
        .optional = true;

    let updated = try_compose_fast_updates(
        &subject,
        &[],
        &[],
        &config,
        patch_hashes(&config).as_ref(),
        false,
    )
    .expect("widening the ignore list needs no resolution");

    assert!(
        snapshot_optional(&updated, "child@3.0.0"),
        "only the optional path reaches child once bar is ignored",
    );
}

/// The hashes the caller computes once per attempt and hands to the
/// pipeline.
fn patch_hashes(config: &Config) -> Option<std::collections::BTreeMap<String, String>> {
    config.patched_dependency_hashes().expect("hash the configured patch files")
}

fn workspace(patches: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().expect("create workspace dir");
    fs::create_dir_all(dir.path().join("patches")).expect("create patches dir");
    for patch in patches {
        let path = dir.path().join("patches").join(patch_file_name(patch));
        fs::write(path, "--- a\n+++ b\n").expect("write patch file");
    }
    dir
}

fn patch_file_name(key: &str) -> String {
    format!("{}.patch", key.replace('/', "+"))
}

fn patch_config(workspace_dir: &Path, keys: &[&str]) -> Config {
    Config {
        workspace_dir: Some(workspace_dir.to_path_buf()),
        allow_unused_patches: true,
        patched_dependencies: (!keys.is_empty()).then(|| {
            keys.iter()
                .map(|key| (key.to_string(), format!("patches/{}", patch_file_name(key))))
                .collect::<IndexMap<_, _>>()
        }),
        ..Config::default()
    }
}

#[test]
fn absorbs_a_peer_setting_once_the_removal_drops_the_last_peer_dependent() {
    let mut subject = lockfile();
    subject.settings =
        Some(crate::fast_update_settings::lockfile_settings_from_config(&Config::default()));
    subject
        .importers
        .get_mut(".")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "has-peer".parse().expect("alias"),
            serde_saphyr::from_str("{specifier: ^6.0.0, version: 6.0.0}").expect("dependency"),
        );
    subject.packages.as_mut().expect("packages").insert(
        "has-peer@6.0.0".parse().expect("package key"),
        serde_saphyr::from_str(
            "resolution:\n  integrity: sha512-has-peer\npeerDependencies:\n  foo: ^1.0.0",
        )
        .expect("package"),
    );
    subject.snapshots.as_mut().expect("snapshots").insert(
        "has-peer@6.0.0".parse().expect("snapshot key"),
        serde_saphyr::from_str("{}").expect("snapshot"),
    );
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0", "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let config = Config { auto_install_peers: false, ..Config::default() };

    let updated = try_compose_fast_updates(
        &subject,
        &[(".".to_string(), &manifest)],
        &[(PathBuf::from("/project"), &manifest)],
        &config,
        patch_hashes(&config).as_ref(),
        false,
    )
    .expect("the removal prunes the only peer dependent, so the setting cannot affect the graph");

    assert!(!updated.settings.as_ref().expect("settings").auto_install_peers);
    assert!(
        !snapshot_keys(&updated).iter().any(|key| key.starts_with("has-peer@")),
        "the peer-declaring package went with the removal",
    );
}
