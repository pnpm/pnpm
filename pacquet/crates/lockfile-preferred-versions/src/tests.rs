use std::{collections::HashMap, str::FromStr};

use pacquet_lockfile::{PackageKey, SnapshotEntry};
use pacquet_package_manifest::PackageManifest;
use pacquet_resolving_resolver_base::{
    DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry,
    VersionSelectorType,
};
use pretty_assertions::assert_eq;

use super::get_preferred_versions_from_lockfile_and_manifests;

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn fake_manifest(deps_json: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(&deps_json).unwrap()).expect("write");
    let manifest = PackageManifest::from_path(path).expect("parse");
    (tmp, manifest)
}

fn weight_of(entry: &VersionSelectorEntry) -> u32 {
    match entry {
        VersionSelectorEntry::Plain(_) => 0,
        VersionSelectorEntry::Weighted(w) => w.weight,
    }
}

fn selector_type_of(entry: &VersionSelectorEntry) -> VersionSelectorType {
    match entry {
        VersionSelectorEntry::Plain(t) => *t,
        VersionSelectorEntry::Weighted(w) => w.selector_type,
    }
}

#[test]
fn seeds_from_manifest_only_when_no_lockfile_snapshots() {
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": { "foo": "^1.0.0", "bar": "2.3.4" },
        "devDependencies": { "baz": "latest" },
    }));

    let preferred = get_preferred_versions_from_lockfile_and_manifests(None, &[&manifest]);

    let foo = preferred.get("foo").expect("foo entry");
    assert_eq!(foo.len(), 1);
    let foo_entry = foo.get("^1.0.0").expect("foo spec entry");
    assert_eq!(selector_type_of(foo_entry), VersionSelectorType::Range);
    assert_eq!(weight_of(foo_entry), DIRECT_DEP_SELECTOR_WEIGHT);

    let bar_entry = preferred.get("bar").unwrap().get("2.3.4").unwrap();
    assert_eq!(selector_type_of(bar_entry), VersionSelectorType::Version);

    let baz_entry = preferred.get("baz").unwrap().get("latest").unwrap();
    assert_eq!(selector_type_of(baz_entry), VersionSelectorType::Tag);
}

#[test]
fn skips_manifest_specs_that_arent_versions_ranges_or_tags() {
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": {
            "good": "^1.0.0",
            "from-git": "git+ssh://example.com/repo.git",
            "from-workspace": "workspace:*",
        },
    }));

    let preferred = get_preferred_versions_from_lockfile_and_manifests(None, &[&manifest]);

    assert!(preferred.contains_key("good"));
    assert!(!preferred.contains_key("from-git"));
    assert!(!preferred.contains_key("from-workspace"));
}

#[test]
fn lockfile_snapshots_seed_existing_version_selectors() {
    let mut snapshots = HashMap::new();
    snapshots.insert(PackageKey::from_str("foo@1.0.0").unwrap(), SnapshotEntry::default());
    let (_tmp, empty) = fake_manifest(serde_json::json!({ "name": "root", "version": "0.0.0" }));

    let preferred = get_preferred_versions_from_lockfile_and_manifests(Some(&snapshots), &[&empty]);

    let entry = preferred.get("foo").unwrap().get("1.0.0").unwrap();
    assert_eq!(selector_type_of(entry), VersionSelectorType::Version);
    assert_eq!(weight_of(entry), EXISTING_VERSION_SELECTOR_WEIGHT);
}

#[test]
fn dual_source_match_bumps_weight() {
    // foo@1.0.0 appears in both manifest (as direct dep) and lockfile (as snapshot).
    // The combined weight should be DIRECT + EXISTING + 1 because the manifest entry
    // started as a `Weighted(DIRECT)` and lockfile adds EXISTING on top (the `+1`
    // path is for `Plain` selectors — see add_weight_to_version_selector).
    let mut snapshots = HashMap::new();
    snapshots.insert(PackageKey::from_str("foo@1.0.0").unwrap(), SnapshotEntry::default());
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": { "foo": "1.0.0" },
    }));

    let preferred =
        get_preferred_versions_from_lockfile_and_manifests(Some(&snapshots), &[&manifest]);

    let entry = preferred.get("foo").unwrap().get("1.0.0").unwrap();
    assert_eq!(selector_type_of(entry), VersionSelectorType::Version);
    assert_eq!(weight_of(entry), DIRECT_DEP_SELECTOR_WEIGHT + EXISTING_VERSION_SELECTOR_WEIGHT);
}

#[test]
fn duplicate_peer_suffix_snapshots_do_not_inflate_weight() {
    // foo@1.0.0 with three different peer suffixes — uniqueNameVersions
    // dedup ensures the weight is added once, not three times.
    let mut snapshots = HashMap::new();
    snapshots.insert(PackageKey::from_str("foo@1.0.0(a@1)").unwrap(), SnapshotEntry::default());
    snapshots.insert(PackageKey::from_str("foo@1.0.0(b@2)").unwrap(), SnapshotEntry::default());
    snapshots.insert(PackageKey::from_str("foo@1.0.0(c@3)").unwrap(), SnapshotEntry::default());
    let (_tmp, empty) = fake_manifest(serde_json::json!({ "name": "root", "version": "0.0.0" }));

    let preferred = get_preferred_versions_from_lockfile_and_manifests(Some(&snapshots), &[&empty]);

    let entry = preferred.get("foo").unwrap().get("1.0.0").unwrap();
    assert_eq!(weight_of(entry), EXISTING_VERSION_SELECTOR_WEIGHT);
}
