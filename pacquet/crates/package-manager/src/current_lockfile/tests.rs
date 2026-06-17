//! Unit tests for [`super::filter_lockfile_for_current`].
//!
//! Cover the four behaviors upstream's `filterLockfileByImportersAndEngine`
//! is documented to produce:
//!
//! 1. Importer dep maps are cleared per `include` flags.
//! 2. Importer `optional_dependencies` is further trimmed to entries
//!    whose target snapshot survives the reachability walk.
//! 3. Snapshots in [`crate::SkippedSnapshots`] are pruned from the
//!    snapshot map, along with their transitive subtrees.
//! 4. A snapshot reachable through any **non-skipped** importer
//!    root survives, even if other importer roots wanted it.

use std::collections::HashMap;

use pacquet_lockfile::{
    ComVer, ImporterDepVersion, Lockfile, LockfileVersion, PackageKey, PkgName, PkgVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pacquet_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;

use crate::SkippedSnapshots;

fn pkg(name: &str) -> PkgName {
    PkgName::parse(name).expect("parse PkgName")
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(pkg(name_text), ver(version))
}

fn importer_dep(version: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec {
        specifier: version.to_string(),
        version: ImporterDepVersion::Regular(ver(version)),
    }
}

fn importer_map(entries: &[(&str, &str)]) -> ResolvedDependencyMap {
    entries.iter().map(|(n, v)| (pkg(n), importer_dep(v))).collect()
}

fn snapshot_with_deps(deps: &[(&str, &str)]) -> SnapshotEntry {
    let map: HashMap<PkgName, SnapshotDepRef> =
        deps.iter().map(|(n, v)| (pkg(n), SnapshotDepRef::Plain(ver(v)))).collect();
    SnapshotEntry { dependencies: Some(map), ..Default::default() }
}

fn empty_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 }).unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    }
}

fn include_all() -> IncludedDependencies {
    IncludedDependencies { dependencies: true, dev_dependencies: true, optional_dependencies: true }
}

/// Skipped snapshots are pruned from `snapshots:`, and the importer
/// `optional_dependencies` entry pointing at the skipped snapshot
/// is dropped. Mirrors the post-filter at upstream's
/// [`filterLockfileByImportersAndEngine.ts:75-83`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/filtering/src/filterLockfileByImportersAndEngine.ts#L75-L83).
#[test]
fn skipped_snapshot_pruned_from_snapshots_and_importer_optional() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("keep", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("drop", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("keep", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("drop", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("drop", "1.0.0"));

    let filtered = super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(snaps.contains_key(&key("keep", "1.0.0")));
    assert!(!snaps.contains_key(&key("drop", "1.0.0")), "skipped snapshot must be pruned");

    let imp = filtered.importers.get(".").unwrap();
    assert!(
        imp.optional_dependencies.as_ref().unwrap().is_empty(),
        "importer optional_dependencies entry pointing at a pruned snapshot must be removed",
    );
    assert!(imp.dependencies.as_ref().unwrap().contains_key(&pkg("keep")));
}

/// `include.optional_dependencies = false` clears the importer's
/// `optional_dependencies` map entirely, and the transitively
/// optional-only snapshots also get pruned because no reachability
/// edge survives. Mirrors upstream's
/// [`filterImporter`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/filtering/src/filterImporter.ts#L4-L16).
#[test]
fn include_optional_false_clears_importer_section() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("keep", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("opt", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("keep", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("opt", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    // Match the install pipeline: when `--no-optional` is passed,
    // `InstallFrozenLockfile::run` also adds optional-only snapshots
    // to `skipped`, so the BFS doesn't reach `opt@1.0.0` via the
    // importer root either.
    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("opt", "1.0.0"));
    let include = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    };

    let filtered = super::filter_lockfile_for_current(&lockfile, include, &skipped);

    assert!(filtered.importers.get(".").unwrap().optional_dependencies.is_none());
    assert!(!filtered.snapshots.as_ref().unwrap().contains_key(&key("opt", "1.0.0")));
    assert!(filtered.snapshots.as_ref().unwrap().contains_key(&key("keep", "1.0.0")));
}

/// A transitive snapshot reachable only through a skipped parent
/// is pruned. The reachability walk short-circuits when it hits a
/// skipped key, so descendants don't get queued.
#[test]
fn transitive_under_skipped_snapshot_is_pruned() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            optional_dependencies: Some(importer_map(&[("parent", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("parent", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")]));
    snapshots.insert(key("child", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("parent", "1.0.0"));

    let filtered = super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(!snaps.contains_key(&key("parent", "1.0.0")));
    assert!(
        !snaps.contains_key(&key("child", "1.0.0")),
        "transitive under a skipped parent must be pruned too",
    );
}

/// A snapshot reachable through both a kept and a skipped path
/// survives. Mirrors the upstream
/// [`optionalDependencies.ts:712`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/optionalDependencies.ts#L712)
/// case at the filter level: `shared` survives because the kept
/// prod path reaches it even when the optional path is dropped.
#[test]
fn snapshot_reachable_via_kept_path_survives() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("kept-parent", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("opt-parent", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("kept-parent", "1.0.0"), snapshot_with_deps(&[("shared", "1.0.0")]));
    snapshots.insert(key("opt-parent", "1.0.0"), snapshot_with_deps(&[("shared", "1.0.0")]));
    snapshots.insert(key("shared", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("opt-parent", "1.0.0"));

    let filtered = super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(snaps.contains_key(&key("kept-parent", "1.0.0")));
    assert!(
        snaps.contains_key(&key("shared", "1.0.0")),
        "shared snapshot must survive because the kept parent still references it",
    );
    assert!(!snaps.contains_key(&key("opt-parent", "1.0.0")));
}

/// `packages:` is filtered to metadata rows backing the surviving
/// snapshots. Peer-variant snapshots collapse to the same metadata
/// key (via `without_peer()`), so a metadata row backing any
/// surviving peer-variant stays. Verifies the
/// `reachable_metadata.insert(snap_key.without_peer())` line in the
/// filter.
#[test]
fn packages_filtered_to_surviving_metadata_keys() {
    use pacquet_lockfile::{LockfileResolution, PackageMetadata, TarballResolution};

    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("keep", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("drop", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("keep", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("drop", "1.0.0"), SnapshotEntry::default());

    let make_metadata = |name: &str| PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: format!("https://example.test/{name}.tgz"),
            git_hosted: None,
            path: None,
        }),
        version: None,
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
    };

    let mut packages = HashMap::new();
    packages.insert(key("keep", "1.0.0"), make_metadata("keep"));
    packages.insert(key("drop", "1.0.0"), make_metadata("drop"));

    let lockfile = Lockfile {
        importers,
        snapshots: Some(snapshots),
        packages: Some(packages),
        ..empty_lockfile()
    };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("drop", "1.0.0"));

    let filtered = super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let pkgs = filtered.packages.as_ref().unwrap();
    assert!(pkgs.contains_key(&key("keep", "1.0.0")));
    assert!(
        !pkgs.contains_key(&key("drop", "1.0.0")),
        "metadata for a pruned snapshot must also be pruned",
    );
}

/// `link:` workspace deps survive the optional-deps post-filter
/// because they have no snapshot to reach (they live as a direct
/// symlink, not in the virtual store). Verifies the
/// `as_regular().is_none()` short-circuit in
/// [`super::retain_reachable`].
#[test]
fn link_optional_entries_survive_post_filter() {
    let mut opt_map = ResolvedDependencyMap::new();
    opt_map.insert(
        pkg("workspace-pkg"),
        ResolvedDependencySpec {
            specifier: "workspace:*".to_string(),
            version: ImporterDepVersion::Link("../workspace-pkg".to_string()),
        },
    );

    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot { optional_dependencies: Some(opt_map), ..Default::default() },
    );

    let lockfile = Lockfile { importers, snapshots: Some(HashMap::new()), ..empty_lockfile() };

    let filtered =
        super::filter_lockfile_for_current(&lockfile, include_all(), &SkippedSnapshots::new());

    let opt = filtered.importers.get(".").unwrap().optional_dependencies.as_ref().unwrap();
    assert!(
        opt.contains_key(&pkg("workspace-pkg")),
        "link: importer entries must survive the optional-deps post-filter",
    );
}

/// Empty `skipped` + all-include: the filter is a near-identity
/// transform — every importer, snapshot, and package survives.
/// Acts as the baseline test that no over-filtering happens in
/// the common case.
#[test]
fn empty_skipped_and_full_include_is_identity_for_reachables() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("a", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("a", "1.0.0"), snapshot_with_deps(&[("b", "1.0.0")]));
    snapshots.insert(key("b", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let filtered =
        super::filter_lockfile_for_current(&lockfile, include_all(), &SkippedSnapshots::new());

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert_eq!(snaps.len(), 2);
    assert!(snaps.contains_key(&key("a", "1.0.0")));
    assert!(snaps.contains_key(&key("b", "1.0.0")));

    let imp = filtered.importers.get(".").unwrap();
    assert!(imp.dependencies.as_ref().unwrap().contains_key(&pkg("a")));
}

/// Orphan snapshots — present in the lockfile's `snapshots:` map
/// but not reachable from any importer — get pruned. Pacquet's
/// filter takes the same "reachable-only" shape upstream's
/// [`pickPkgsWithAllDeps`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/filtering/src/filterLockfileByImportersAndEngine.ts#L96-L150)
/// does, since the BFS roots are importer-derived.
#[test]
fn orphan_snapshots_are_pruned() {
    let importers = HashMap::from([(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("a", "1.0.0")])),
            ..Default::default()
        },
    )]);

    let mut snapshots = HashMap::new();
    snapshots.insert(key("a", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("orphan", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let filtered =
        super::filter_lockfile_for_current(&lockfile, include_all(), &SkippedSnapshots::new());

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert_eq!(snaps.len(), 1);
    assert!(snaps.contains_key(&key("a", "1.0.0")));
    assert!(!snaps.contains_key(&key("orphan", "1.0.0")));
}
