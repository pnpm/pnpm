use pnpm_lockfile::{Lockfile, LockfileSettings};
use pnpm_package_manifest::PackageManifest;
use serde_json::{Value, json};
use std::path::PathBuf;

/// Detection and application chained the way the composed pipeline
/// chains them, with an arbitrary `settings` block instead of one
/// derived from a `Config`.
fn try_fast_update_settings(
    lockfile: &Lockfile,
    settings: &LockfileSettings,
    manifests: &[(PathBuf, &PackageManifest)],
) -> Option<Lockfile> {
    match super::detect_settings_drift(lockfile, settings) {
        crate::fast_update_compose::Drift::Absorb(()) => {
            let mut candidate = lockfile.clone();
            super::apply_settings_update(&mut candidate, settings, manifests).then_some(candidate)
        }
        _ => None,
    }
}

const PEERLESS_LOCKFILE: &str = r"
lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  foo@1.1.0: {}
";

fn lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
}

fn manifest(value: Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("/project/package.json"), value)
}

fn recorded_settings() -> LockfileSettings {
    LockfileSettings {
        auto_install_peers: true,
        dedupe_peers: None,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
    }
}

#[test]
fn records_every_setting_the_locked_graph_cannot_notice() {
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let settings = LockfileSettings {
        auto_install_peers: false,
        dedupe_peers: Some(true),
        exclude_links_from_lockfile: true,
        inject_workspace_packages: true,
        peers_suffix_max_length: Some(10),
    };

    let updated = try_fast_update_settings(
        &lockfile(PEERLESS_LOCKFILE),
        &settings,
        &[(PathBuf::from("/project"), &manifest)],
    )
    .expect("a peerless, linkless lockfile absorbs every setting change");
    assert_eq!(updated.settings, Some(settings));
}

#[test]
fn rejects_a_peer_setting_when_a_package_declares_peer_dependencies() {
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
    peerDependencies:
      bar: ^1.0.0
snapshots:
  foo@1.1.0: {}
",
    );
    let settings = LockfileSettings { dedupe_peers: Some(true), ..recorded_settings() };

    assert!(
        try_fast_update_settings(&lockfile, &settings, &[(PathBuf::from("/project"), &manifest)])
            .is_none(),
    );
}

#[test]
fn rejects_a_peer_setting_when_a_snapshot_key_carries_a_peers_suffix() {
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0(bar@1.0.0)
packages:
  foo@1.1.0:
    resolution:
      integrity: sha512-deadbeef
snapshots:
  foo@1.1.0(bar@1.0.0): {}
",
    );
    let settings = LockfileSettings { peers_suffix_max_length: Some(10), ..recorded_settings() };

    assert!(
        try_fast_update_settings(&lockfile, &settings, &[(PathBuf::from("/project"), &manifest)])
            .is_none(),
    );
}

#[test]
fn rejects_a_peer_setting_when_a_project_declares_peer_dependencies() {
    let manifest = manifest(json!({
        "dependencies": { "foo": "^1.0.0" },
        "peerDependencies": { "bar": "^1.0.0" },
    }));
    let settings = LockfileSettings { auto_install_peers: false, ..recorded_settings() };

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &settings,
            &[(PathBuf::from("/project"), &manifest)],
        )
        .is_none(),
    );
}

#[test]
fn rejects_exclude_links_when_a_project_depends_on_a_directory() {
    let manifest = manifest(json!({ "dependencies": { "bar": "link:../bar", "foo": "^1.0.0" } }));
    let settings = LockfileSettings { exclude_links_from_lockfile: true, ..recorded_settings() };

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &settings,
            &[(PathBuf::from("/project"), &manifest)],
        )
        .is_none(),
    );
}

#[test]
fn rejects_exclude_links_when_the_lockfile_records_a_link() {
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    dependencies:
      bar:
        specifier: link:../bar
        version: link:../bar
",
    );
    let settings = LockfileSettings { exclude_links_from_lockfile: true, ..recorded_settings() };

    assert!(
        try_fast_update_settings(&lockfile, &settings, &[(PathBuf::from("/project"), &manifest)])
            .is_none(),
    );
}

#[test]
fn records_exclude_links_when_the_only_workspace_dependency_uses_the_workspace_protocol() {
    let root = manifest(json!({
        "name": "root",
        "dependencies": { "bar": "workspace:*", "foo": "^1.0.0" },
    }));
    let sibling = manifest(json!({ "name": "bar" }));
    let settings = LockfileSettings { exclude_links_from_lockfile: true, ..recorded_settings() };

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &settings,
            &[(PathBuf::from("/project"), &root), (PathBuf::from("/project/bar"), &sibling)],
        )
        .is_some(),
    );
}

#[test]
fn rejects_exclude_links_when_a_workspace_project_is_depended_on_by_range() {
    let root =
        manifest(json!({ "name": "root", "dependencies": { "bar": "^1.0.0", "foo": "^1.0.0" } }));
    let sibling = manifest(json!({ "name": "bar" }));
    let settings = LockfileSettings { exclude_links_from_lockfile: true, ..recorded_settings() };

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &settings,
            &[(PathBuf::from("/project"), &root), (PathBuf::from("/project/bar"), &sibling)],
        )
        .is_none(),
    );
}

#[test]
fn rejects_inject_workspace_packages_when_a_workspace_project_is_depended_on() {
    let root = manifest(json!({
        "name": "root",
        "dependencies": { "bar": "workspace:*", "foo": "^1.0.0" },
    }));
    let sibling = manifest(json!({ "name": "bar" }));
    let settings = LockfileSettings { inject_workspace_packages: true, ..recorded_settings() };

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &settings,
            &[(PathBuf::from("/project"), &root), (PathBuf::from("/project/bar"), &sibling)],
        )
        .is_none(),
    );
}

#[test]
fn rejects_inject_workspace_packages_when_a_dependency_is_already_injected() {
    let manifest = manifest(json!({
        "dependencies": { "foo": "^1.0.0" },
        "dependenciesMeta": { "foo": { "injected": true } },
    }));
    let settings = LockfileSettings { inject_workspace_packages: true, ..recorded_settings() };

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &settings,
            &[(PathBuf::from("/project"), &manifest)],
        )
        .is_none(),
    );
}

#[test]
fn rejects_a_group_of_changed_settings_when_one_of_them_is_unsafe() {
    let manifest = manifest(json!({ "dependencies": { "bar": "link:../bar", "foo": "^1.0.0" } }));
    let settings = LockfileSettings {
        dedupe_peers: Some(true),
        inject_workspace_packages: true,
        ..recorded_settings()
    };

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &settings,
            &[(PathBuf::from("/project"), &manifest)],
        )
        .is_none(),
    );
}

#[test]
fn reports_no_update_when_the_settings_match() {
    let manifest = manifest(json!({ "dependencies": { "foo": "^1.0.0" } }));

    assert!(
        try_fast_update_settings(
            &lockfile(PEERLESS_LOCKFILE),
            &recorded_settings(),
            &[(PathBuf::from("/project"), &manifest)],
        )
        .is_none(),
    );
}
