//! Unit tests for [`super::filter_lockfile_for_current`].

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use indexmap::IndexMap;
use pacquet_lockfile::{
    CatalogSnapshots, ComVer, ImporterDepVersion, Lockfile, LockfileResolution, LockfileSettings,
    LockfileVersion, PackageKey, PackageMetadata, PkgName, PkgVerPeer, ProjectSnapshot,
    ResolvedCatalogEntry, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef,
    SnapshotEntry, TarballResolution,
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

fn importer_link(target: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec {
        specifier: "workspace:*".to_string(),
        version: ImporterDepVersion::Link(target.to_string()),
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

fn package_metadata(name: &str) -> PackageMetadata {
    PackageMetadata {
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
    }
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

fn lockfile_with_top_level(marker: &str, minor: u16) -> Lockfile {
    let catalogs = CatalogSnapshots::from([(
        "default".to_string(),
        BTreeMap::from([(
            "catalog-pkg".to_string(),
            ResolvedCatalogEntry {
                specifier: format!("{marker}-specifier"),
                version: format!("{marker}-version"),
            },
        )]),
    )]);
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor }).unwrap(),
        settings: Some(LockfileSettings {
            auto_install_peers: marker == "fresh",
            dedupe_peers: Some(marker == "fresh"),
            exclude_links_from_lockfile: marker != "fresh",
            inject_workspace_packages: marker == "fresh",
            peers_suffix_max_length: Some(if marker == "fresh" { 2000 } else { 1000 }),
        }),
        catalogs: Some(catalogs),
        overrides: Some(IndexMap::from([(
            "override-pkg".to_string(),
            format!("{marker}-override"),
        )])),
        package_extensions_checksum: Some(format!("{marker}-extensions")),
        pnpmfile_checksum: Some(format!("{marker}-pnpmfile")),
        ignored_optional_dependencies: Some(vec![format!("{marker}-ignored")]),
        patched_dependencies: Some(BTreeMap::from([(
            "patched@1.0.0".to_string(),
            format!("{marker}-patch"),
        )])),
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    }
}

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

#[test]
fn user_excluded_packages_filtered_to_surviving_metadata_keys() {
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

    let mut packages = HashMap::new();
    packages.insert(key("keep", "1.0.0"), package_metadata("keep"));
    packages.insert(key("drop", "1.0.0"), package_metadata("drop"));

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
        "metadata for a user-excluded snapshot must also be pruned",
    );
}

#[test]
fn installability_skipped_metadata_is_preserved() {
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
    snapshots.insert(key("drop", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")]));
    snapshots.insert(key("child", "1.0.0"), SnapshotEntry::default());

    let mut packages = HashMap::new();
    packages.insert(key("keep", "1.0.0"), package_metadata("keep"));
    packages.insert(key("drop", "1.0.0"), package_metadata("drop"));
    packages.insert(key("child", "1.0.0"), package_metadata("child"));

    let lockfile = Lockfile {
        importers,
        snapshots: Some(snapshots),
        packages: Some(packages),
        ..empty_lockfile()
    };

    let mut skipped = SkippedSnapshots::new();
    skipped.insert_installability(key("drop", "1.0.0"));

    let filtered = super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(snaps.contains_key(&key("keep", "1.0.0")));
    assert!(!snaps.contains_key(&key("drop", "1.0.0")));
    assert!(!snaps.contains_key(&key("child", "1.0.0")));

    let pkgs = filtered.packages.as_ref().unwrap();
    assert!(pkgs.contains_key(&key("keep", "1.0.0")));
    assert!(pkgs.contains_key(&key("drop", "1.0.0")));
    assert!(pkgs.contains_key(&key("child", "1.0.0")));
}

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

#[test]
fn materialization_closure_traverses_importer_snapshot_and_link_cycles() {
    let nested_id = "packages/nested/a".to_string();
    let linked_id = "packages/b".to_string();
    let shared_id = "packages/shared".to_string();
    let disjoint_id = "packages/disjoint".to_string();

    let mut nested_deps = importer_map(&[("start", "1.0.0")]);
    nested_deps.insert(pkg("linked-workspace"), importer_link("../../../packages/b"));
    let mut linked_deps = importer_map(&[("linked-pkg", "1.0.0")]);
    linked_deps.insert(pkg("back-to-start"), importer_link("../nested/a"));

    let importers = HashMap::from([
        (
            nested_id.clone(),
            ProjectSnapshot {
                dependencies: Some(nested_deps),
                dev_dependencies: Some(importer_map(&[("dev-only", "1.0.0")])),
                optional_dependencies: Some(importer_map(&[("optional-only", "1.0.0")])),
                ..Default::default()
            },
        ),
        (
            linked_id.clone(),
            ProjectSnapshot { dependencies: Some(linked_deps), ..Default::default() },
        ),
        (
            shared_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("shared-pkg", "1.0.0")])),
                ..Default::default()
            },
        ),
        (
            disjoint_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("disjoint", "1.0.0")])),
                ..Default::default()
            },
        ),
    ]);

    let snapshots = HashMap::from([
        (
            key("start", "1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([
                    (pkg("child"), SnapshotDepRef::Plain(ver("1.0.0"))),
                    (pkg("common"), SnapshotDepRef::Plain(ver("1.0.0"))),
                    (pkg("shared-workspace"), SnapshotDepRef::Link("packages/shared".to_string())),
                ])),
                ..Default::default()
            },
        ),
        (key("child", "1.0.0"), snapshot_with_deps(&[("start", "1.0.0")])),
        (key("linked-pkg", "1.0.0"), snapshot_with_deps(&[("common", "1.0.0")])),
        (key("common", "1.0.0"), SnapshotEntry::default()),
        (
            key("shared-pkg", "1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([(
                    pkg("back-to-nested"),
                    SnapshotDepRef::Link("packages/nested/a".to_string()),
                )])),
                ..Default::default()
            },
        ),
        (key("dev-only", "1.0.0"), SnapshotEntry::default()),
        (key("optional-only", "1.0.0"), SnapshotEntry::default()),
        (key("disjoint", "1.0.0"), SnapshotEntry::default()),
    ]);
    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };
    let selected = HashSet::from([nested_id.clone()]);
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    let closure = super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &selected,
        included,
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([nested_id.clone(), linked_id, shared_id]));
    assert!(!closure.importer_ids.contains(&disjoint_id));
    let nested = closure.lockfile.importers.get(&nested_id).unwrap();
    assert!(nested.dev_dependencies.is_none());
    assert!(nested.optional_dependencies.is_none());
    let reached = closure.lockfile.snapshots.as_ref().unwrap();
    for reached_key in [
        key("start", "1.0.0"),
        key("child", "1.0.0"),
        key("linked-pkg", "1.0.0"),
        key("common", "1.0.0"),
        key("shared-pkg", "1.0.0"),
    ] {
        assert!(reached.contains_key(&reached_key), "missing {reached_key}");
    }
    assert!(!reached.contains_key(&key("dev-only", "1.0.0")));
    assert!(!reached.contains_key(&key("optional-only", "1.0.0")));
    assert!(!reached.contains_key(&key("disjoint", "1.0.0")));
}

#[test]
fn materialization_closure_excludes_transitive_optional_shared_snapshot_when_disabled() {
    let selected_id = "packages/selected".to_string();
    let unselected_id = "packages/unselected".to_string();
    let lockfile = Lockfile {
        importers: HashMap::from([
            (
                selected_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("parent", "1.0.0")])),
                    ..Default::default()
                },
            ),
            (
                unselected_id,
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("shared", "1.0.0")])),
                    ..Default::default()
                },
            ),
        ]),
        snapshots: Some(HashMap::from([
            (
                key("parent", "1.0.0"),
                SnapshotEntry {
                    optional_dependencies: Some(HashMap::from([(
                        pkg("shared"),
                        SnapshotDepRef::Plain(ver("1.0.0")),
                    )])),
                    ..Default::default()
                },
            ),
            (key("shared", "1.0.0"), SnapshotEntry::default()),
        ])),
        ..empty_lockfile()
    };
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    };

    let closure = super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([selected_id.clone()]),
        included,
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([selected_id]));
    let snapshots = closure.lockfile.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("parent", "1.0.0")));
    assert!(!snapshots.contains_key(&key("shared", "1.0.0")));
}

#[test]
fn materialization_closure_excludes_optional_snapshot_link_when_optionals_are_disabled() {
    let selected_id = "packages/selected".to_string();
    let linked_id = "packages/linked".to_string();
    let lockfile = Lockfile {
        importers: HashMap::from([
            (
                selected_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("parent", "1.0.0")])),
                    ..Default::default()
                },
            ),
            (
                linked_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("linked-pkg", "1.0.0")])),
                    ..Default::default()
                },
            ),
        ]),
        snapshots: Some(HashMap::from([
            (
                key("parent", "1.0.0"),
                SnapshotEntry {
                    optional_dependencies: Some(HashMap::from([(
                        pkg("linked"),
                        SnapshotDepRef::Link("packages/linked".to_string()),
                    )])),
                    ..Default::default()
                },
            ),
            (key("linked-pkg", "1.0.0"), SnapshotEntry::default()),
        ])),
        ..empty_lockfile()
    };
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    };

    let closure = super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([selected_id.clone()]),
        included,
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([selected_id]));
    assert!(!closure.importer_ids.contains(&linked_id));
    let snapshots = closure.lockfile.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("parent", "1.0.0")));
    assert!(!snapshots.contains_key(&key("linked-pkg", "1.0.0")));
}

#[test]
fn nested_importer_and_peer_snapshot_links_use_distinct_bases() {
    let nested_id = "packages/nested/a".to_string();
    let importer_target_id = "packages/importer-target".to_string();
    let snapshot_target_id = "packages/snapshot-target".to_string();
    let provider_version = "1.0.0(peer@2.0.0)";
    let mut dependencies = importer_map(&[("provider", provider_version)]);
    dependencies.insert(pkg("importer-target"), importer_link("../../../packages/importer-target"));
    let importers = HashMap::from([
        (
            nested_id.clone(),
            ProjectSnapshot { dependencies: Some(dependencies), ..Default::default() },
        ),
        (importer_target_id.clone(), ProjectSnapshot::default()),
        (snapshot_target_id.clone(), ProjectSnapshot::default()),
    ]);
    let snapshots = HashMap::from([(
        key("provider", provider_version),
        SnapshotEntry {
            dependencies: Some(HashMap::from([(
                pkg("snapshot-target"),
                SnapshotDepRef::Link("packages/snapshot-target".to_string()),
            )])),
            ..Default::default()
        },
    )]);
    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let closure = super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([nested_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(
        closure.importer_ids,
        HashSet::from([nested_id, importer_target_id, snapshot_target_id])
    );
}

#[test]
fn materialization_closure_ignores_unknown_link_targets() {
    let mut dependencies = importer_map(&[("parent", "1.0.0")]);
    dependencies.insert(pkg("outside"), importer_link("../outside"));
    let importers = HashMap::from([
        (
            Lockfile::ROOT_IMPORTER_KEY.to_string(),
            ProjectSnapshot { dependencies: Some(dependencies), ..Default::default() },
        ),
        ("packages/known".to_string(), ProjectSnapshot::default()),
    ]);
    let snapshots = HashMap::from([(
        key("parent", "1.0.0"),
        SnapshotEntry {
            dependencies: Some(HashMap::from([(
                pkg("missing"),
                SnapshotDepRef::Link("packages/missing".to_string()),
            )])),
            ..Default::default()
        },
    )]);
    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let closure = super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([Lockfile::ROOT_IMPORTER_KEY.to_string()]),
        include_all(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([Lockfile::ROOT_IMPORTER_KEY.to_string()]));
    assert_eq!(closure.lockfile.importers.len(), 1);
}

#[test]
fn materialization_closure_does_not_follow_reverse_workspace_links() {
    let selected_id = "packages/shared".to_string();
    let dependent_id = "packages/app".to_string();
    let mut dependent_dependencies = importer_map(&[("app-only", "1.0.0")]);
    dependent_dependencies.insert(pkg("shared"), importer_link("../shared"));
    let lockfile = Lockfile {
        importers: HashMap::from([
            (
                selected_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("shared-only", "1.0.0")])),
                    ..Default::default()
                },
            ),
            (
                dependent_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(dependent_dependencies),
                    ..Default::default()
                },
            ),
        ]),
        snapshots: Some(HashMap::from([
            (key("shared-only", "1.0.0"), SnapshotEntry::default()),
            (key("app-only", "1.0.0"), SnapshotEntry::default()),
        ])),
        ..empty_lockfile()
    };

    let closure = super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([selected_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([selected_id]));
    assert!(!closure.importer_ids.contains(&dependent_id));
    let snapshots = closure.lockfile.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("shared-only", "1.0.0")));
    assert!(!snapshots.contains_key(&key("app-only", "1.0.0")));
}

#[test]
fn merge_filtered_wanted_lockfile_refreshes_all_importers_when_global_inputs_change() {
    let selected_id = "packages/selected".to_string();
    let retained_id = "packages/retained".to_string();
    let removed_id = "packages/removed".to_string();
    let new_id = "packages/new".to_string();

    let prior_retained = ProjectSnapshot {
        specifiers: Some(HashMap::from([("retained".to_string(), "prior".to_string())])),
        dependencies: Some(importer_map(&[("retained", "1.0.0"), ("shared", "1.0.0")])),
        ..Default::default()
    };
    let mut previous = lockfile_with_top_level("prior", 0);
    previous.importers = HashMap::from([
        (
            selected_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("selected-old", "1.0.0")])),
                ..Default::default()
            },
        ),
        (retained_id.clone(), prior_retained),
        (
            removed_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("removed", "1.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    previous.snapshots = Some(HashMap::from([
        (key("selected-old", "1.0.0"), SnapshotEntry::default()),
        (key("retained", "1.0.0"), snapshot_with_deps(&[("retained-child", "1.0.0")])),
        (key("retained-child", "1.0.0"), SnapshotEntry::default()),
        (key("shared", "1.0.0"), snapshot_with_deps(&[("old-child", "1.0.0")])),
        (key("old-child", "1.0.0"), SnapshotEntry::default()),
        (key("removed", "1.0.0"), SnapshotEntry::default()),
    ]));
    previous.packages = Some(
        previous
            .snapshots
            .as_ref()
            .unwrap()
            .keys()
            .map(|package_key| {
                (package_key.without_peer(), package_metadata(&format!("prior-{package_key}")))
            })
            .collect(),
    );

    let fresh_selected = ProjectSnapshot {
        specifiers: Some(HashMap::from([("selected-new".to_string(), "fresh".to_string())])),
        dependencies: Some(importer_map(&[("selected-new", "2.0.0"), ("shared", "1.0.0")])),
        ..Default::default()
    };
    let fresh_new = ProjectSnapshot {
        dependencies: Some(importer_map(&[("new-pkg", "1.0.0")])),
        ..Default::default()
    };
    let mut fresh = lockfile_with_top_level("fresh", 1);
    let fresh_retained = ProjectSnapshot {
        dependencies: Some(importer_map(&[("fresh-only", "2.0.0")])),
        ..Default::default()
    };
    fresh.importers = HashMap::from([
        (selected_id.clone(), fresh_selected.clone()),
        (retained_id.clone(), fresh_retained.clone()),
        (new_id.clone(), fresh_new.clone()),
    ]);
    let fresh_shared = snapshot_with_deps(&[("fresh-child", "2.0.0")]);
    fresh.snapshots = Some(HashMap::from([
        (key("selected-new", "2.0.0"), SnapshotEntry::default()),
        (key("shared", "1.0.0"), fresh_shared.clone()),
        (key("fresh-child", "2.0.0"), SnapshotEntry::default()),
        (key("fresh-only", "2.0.0"), SnapshotEntry::default()),
        (key("new-pkg", "1.0.0"), SnapshotEntry::default()),
    ]));
    fresh.packages = Some(
        fresh
            .snapshots
            .as_ref()
            .unwrap()
            .keys()
            .map(|package_key| {
                (package_key.without_peer(), package_metadata(&format!("fresh-{package_key}")))
            })
            .collect(),
    );
    let expected_lockfile_version = fresh.lockfile_version;
    let expected_settings = fresh.settings.clone();
    let expected_catalogs = fresh.catalogs.clone();
    let expected_overrides = fresh.overrides.clone();
    let expected_package_extensions_checksum = fresh.package_extensions_checksum.clone();
    let expected_pnpmfile_checksum = fresh.pnpmfile_checksum.clone();
    let expected_ignored_optional_dependencies = fresh.ignored_optional_dependencies.clone();
    let expected_patched_dependencies = fresh.patched_dependencies.clone();
    let expected_shared_metadata =
        fresh.packages.as_ref().unwrap().get(&key("shared", "1.0.0")).cloned();

    let merged = super::merge_filtered_wanted_lockfile(
        Some(&previous),
        fresh,
        &HashSet::from([selected_id.clone(), retained_id.clone(), new_id.clone()]),
        &HashSet::from([selected_id.clone()]),
        Path::new("/workspace"),
    )
    .expect("the fresh lockfile contains every required importer");

    assert_eq!(merged.importers.get(&selected_id), Some(&fresh_selected));
    assert_eq!(merged.importers.get(&retained_id), Some(&fresh_retained));
    assert_eq!(merged.importers.get(&new_id), Some(&fresh_new));
    assert!(!merged.importers.contains_key(&removed_id));
    let snapshots = merged.snapshots.as_ref().unwrap();
    assert!(!snapshots.contains_key(&key("retained", "1.0.0")));
    assert!(!snapshots.contains_key(&key("retained-child", "1.0.0")));
    assert!(snapshots.contains_key(&key("fresh-only", "2.0.0")));
    assert!(!snapshots.contains_key(&key("selected-old", "1.0.0")));
    assert_eq!(snapshots.get(&key("shared", "1.0.0")), Some(&fresh_shared));
    assert!(snapshots.contains_key(&key("fresh-child", "2.0.0")));
    assert!(!snapshots.contains_key(&key("old-child", "1.0.0")));
    assert_eq!(
        merged.packages.as_ref().unwrap().get(&key("shared", "1.0.0")),
        expected_shared_metadata.as_ref()
    );
    assert_eq!(merged.lockfile_version, expected_lockfile_version);
    assert_eq!(merged.settings, expected_settings);
    assert_eq!(merged.catalogs, expected_catalogs);
    assert_eq!(merged.overrides, expected_overrides);
    assert_eq!(merged.package_extensions_checksum, expected_package_extensions_checksum);
    assert_eq!(merged.pnpmfile_checksum, expected_pnpmfile_checksum);
    assert_eq!(merged.ignored_optional_dependencies, expected_ignored_optional_dependencies);
    assert_eq!(merged.patched_dependencies, expected_patched_dependencies);
}

#[test]
fn merge_filtered_wanted_lockfile_preserves_unselected_importers_when_global_inputs_match() {
    let selected_id = "packages/selected".to_string();
    let retained_id = "packages/retained".to_string();
    let prior_retained = ProjectSnapshot {
        dependencies: Some(importer_map(&[("retained-old", "1.0.0")])),
        ..Default::default()
    };
    let mut previous = lockfile_with_top_level("same", 0);
    previous.importers = HashMap::from([
        (
            selected_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("selected-old", "1.0.0")])),
                ..Default::default()
            },
        ),
        (retained_id.clone(), prior_retained.clone()),
    ]);
    previous.snapshots = Some(HashMap::from([
        (key("selected-old", "1.0.0"), SnapshotEntry::default()),
        (key("retained-old", "1.0.0"), SnapshotEntry::default()),
    ]));

    let fresh_selected = ProjectSnapshot {
        dependencies: Some(importer_map(&[("selected-new", "2.0.0")])),
        ..Default::default()
    };
    let mut fresh = lockfile_with_top_level("same", 0);
    fresh.importers = HashMap::from([
        (selected_id.clone(), fresh_selected.clone()),
        (
            retained_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("retained-fresh", "2.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    fresh.snapshots = Some(HashMap::from([
        (key("selected-new", "2.0.0"), SnapshotEntry::default()),
        (key("retained-fresh", "2.0.0"), SnapshotEntry::default()),
    ]));

    let merged = super::merge_filtered_wanted_lockfile(
        Some(&previous),
        fresh,
        &HashSet::from([selected_id.clone(), retained_id.clone()]),
        &HashSet::from([selected_id.clone()]),
        Path::new("/workspace"),
    )
    .expect("fresh lockfile contains every real importer");

    assert_eq!(merged.importers.get(&selected_id), Some(&fresh_selected));
    assert_eq!(merged.importers.get(&retained_id), Some(&prior_retained));
    let snapshots = merged.snapshots.as_ref().expect("merged snapshots");
    assert!(snapshots.contains_key(&key("selected-new", "2.0.0")));
    assert!(snapshots.contains_key(&key("retained-old", "1.0.0")));
    assert!(!snapshots.contains_key(&key("selected-old", "1.0.0")));
    assert!(!snapshots.contains_key(&key("retained-fresh", "2.0.0")));
}

#[test]
fn merge_filtered_wanted_lockfile_rejects_a_missing_selected_importer() {
    let error = super::merge_filtered_wanted_lockfile(
        None,
        empty_lockfile(),
        &HashSet::from(["packages/selected".to_string()]),
        &HashSet::from(["packages/selected".to_string()]),
        Path::new("/workspace"),
    )
    .expect_err("a selected importer missing from the fresh lockfile must be rejected");

    assert_eq!(error.to_string(), "fresh lockfile is missing importer packages/selected");
}

#[test]
fn merge_filtered_current_lockfile_preserves_prior_importers_across_sequential_runs() {
    let first_id = "packages/first".to_string();
    let second_id = "packages/second".to_string();
    let mut previous = lockfile_with_top_level("prior", 0);
    let prior_first = ProjectSnapshot {
        dependencies: Some(importer_map(&[("first-old", "1.0.0")])),
        ..Default::default()
    };
    previous.importers = HashMap::from([
        (first_id.clone(), prior_first.clone()),
        (
            second_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("second-old", "1.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    previous.snapshots = Some(HashMap::from([
        (key("first-old", "1.0.0"), SnapshotEntry::default()),
        (key("second-old", "1.0.0"), SnapshotEntry::default()),
    ]));

    let mut wanted = lockfile_with_top_level("fresh", 1);
    let fresh_second = ProjectSnapshot {
        dependencies: Some(importer_map(&[("second-new", "2.0.0")])),
        ..Default::default()
    };
    wanted.importers = HashMap::from([
        (
            first_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("fresh-only-first", "2.0.0")])),
                ..Default::default()
            },
        ),
        (second_id.clone(), fresh_second.clone()),
    ]);
    wanted.snapshots = Some(HashMap::from([
        (key("fresh-only-first", "2.0.0"), SnapshotEntry::default()),
        (key("second-new", "2.0.0"), SnapshotEntry::default()),
    ]));
    let expected_settings = wanted.settings.clone();

    let merged = super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([second_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
        Path::new("/workspace"),
    );

    assert_eq!(merged.importers.get(&first_id), Some(&prior_first));
    assert_eq!(merged.importers.get(&second_id), Some(&fresh_second));
    let snapshots = merged.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("first-old", "1.0.0")));
    assert!(snapshots.contains_key(&key("second-new", "2.0.0")));
    assert!(!snapshots.contains_key(&key("second-old", "1.0.0")));
    assert!(!snapshots.contains_key(&key("fresh-only-first", "2.0.0")));
    assert_eq!(merged.settings, expected_settings);
}

#[test]
fn merge_filtered_current_lockfile_does_not_restore_a_skipped_selected_snapshot() {
    let selected_id = "packages/selected".to_string();
    let selected_importer = ProjectSnapshot {
        dependencies: Some(importer_map(&[("parent", "1.0.0")])),
        ..Default::default()
    };
    let snapshots = HashMap::from([
        (key("parent", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")])),
        (key("child", "1.0.0"), SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (key("parent", "1.0.0"), package_metadata("parent")),
        (key("child", "1.0.0"), package_metadata("child")),
    ]);
    let previous = Lockfile {
        importers: HashMap::from([(selected_id.clone(), selected_importer.clone())]),
        snapshots: Some(snapshots.clone()),
        packages: Some(packages.clone()),
        ..empty_lockfile()
    };
    let wanted = Lockfile {
        importers: HashMap::from([(selected_id.clone(), selected_importer)]),
        snapshots: Some(snapshots),
        packages: Some(packages),
        ..empty_lockfile()
    };
    let mut skipped = SkippedSnapshots::new();
    skipped.insert_installability(key("parent", "1.0.0"));

    let merged = super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([selected_id]),
        include_all(),
        &skipped,
        Path::new("/workspace"),
    );

    let snapshots = merged.snapshots.as_ref().unwrap();
    assert!(!snapshots.contains_key(&key("parent", "1.0.0")));
    assert!(!snapshots.contains_key(&key("child", "1.0.0")));
    let packages = merged.packages.as_ref().unwrap();
    assert!(packages.contains_key(&key("parent", "1.0.0")));
    assert!(packages.contains_key(&key("child", "1.0.0")));
}

#[test]
fn merge_filtered_current_lockfile_uses_one_fresh_shared_snapshot() {
    let retained_id = "packages/retained".to_string();
    let selected_id = "packages/selected".to_string();
    let mut previous = empty_lockfile();
    previous.importers = HashMap::from([(
        retained_id.clone(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("shared", "1.0.0")])),
            ..Default::default()
        },
    )]);
    previous.snapshots = Some(HashMap::from([
        (key("shared", "1.0.0"), snapshot_with_deps(&[("old-child", "1.0.0")])),
        (key("old-child", "1.0.0"), SnapshotEntry::default()),
    ]));
    previous.packages = Some(HashMap::from([
        (key("shared", "1.0.0"), package_metadata("old-shared")),
        (key("old-child", "1.0.0"), package_metadata("old-child")),
    ]));

    let mut wanted = empty_lockfile();
    wanted.importers = HashMap::from([(
        selected_id.clone(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("shared", "1.0.0")])),
            ..Default::default()
        },
    )]);
    let fresh_shared = snapshot_with_deps(&[("fresh-child", "2.0.0")]);
    let fresh_shared_metadata = package_metadata("fresh-shared");
    wanted.snapshots = Some(HashMap::from([
        (key("shared", "1.0.0"), fresh_shared.clone()),
        (key("fresh-child", "2.0.0"), SnapshotEntry::default()),
    ]));
    wanted.packages = Some(HashMap::from([
        (key("shared", "1.0.0"), fresh_shared_metadata.clone()),
        (key("fresh-child", "2.0.0"), package_metadata("fresh-child")),
    ]));

    let merged = super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([selected_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
        Path::new("/workspace"),
    );

    assert!(merged.importers.contains_key(&retained_id));
    assert!(merged.importers.contains_key(&selected_id));
    let snapshots = merged.snapshots.as_ref().unwrap();
    assert_eq!(snapshots.get(&key("shared", "1.0.0")), Some(&fresh_shared));
    assert!(snapshots.contains_key(&key("fresh-child", "2.0.0")));
    assert!(!snapshots.contains_key(&key("old-child", "1.0.0")));
    assert_eq!(
        merged.packages.as_ref().unwrap().get(&key("shared", "1.0.0")),
        Some(&fresh_shared_metadata)
    );
}

#[test]
fn merge_filtered_current_lockfile_replaces_link_reached_importers() {
    let selected_id = "packages/a".to_string();
    let linked_id = "packages/b".to_string();
    let mut previous = empty_lockfile();
    previous.importers = HashMap::from([(
        linked_id.clone(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("linked-old", "1.0.0")])),
            ..Default::default()
        },
    )]);
    previous.snapshots =
        Some(HashMap::from([(key("linked-old", "1.0.0"), SnapshotEntry::default())]));

    let mut selected_dependencies = ResolvedDependencyMap::new();
    selected_dependencies.insert(pkg("linked"), importer_link("../b"));
    let fresh_linked = ProjectSnapshot {
        dependencies: Some(importer_map(&[("linked-new", "2.0.0")])),
        ..Default::default()
    };
    let mut wanted = empty_lockfile();
    wanted.importers = HashMap::from([
        (
            selected_id.clone(),
            ProjectSnapshot { dependencies: Some(selected_dependencies), ..Default::default() },
        ),
        (linked_id.clone(), fresh_linked.clone()),
    ]);
    wanted.snapshots =
        Some(HashMap::from([(key("linked-new", "2.0.0"), SnapshotEntry::default())]));

    let merged = super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([selected_id]),
        include_all(),
        &SkippedSnapshots::new(),
        Path::new("/workspace"),
    );

    assert_eq!(merged.importers.get(&linked_id), Some(&fresh_linked));
    assert!(merged.snapshots.as_ref().unwrap().contains_key(&key("linked-new", "2.0.0")));
    assert!(!merged.snapshots.as_ref().unwrap().contains_key(&key("linked-old", "1.0.0")));
}
