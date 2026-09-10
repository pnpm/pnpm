use super::{
    Arc, BTreeMap, DependencyGroup, HashMap, LockfileResolution, Mutex, RecordingResolver,
    SlowAliasResolver, WorkspaceImporter, fake_manifest, fake_result, importer_opts,
    resolve_workspace, workspace_opts,
};
use std::str::FromStr;

pub(super) fn importer_scoped_update_lockfile(
    importer_ids: &[&str],
    direct_name: &str,
    direct_specifier: &str,
    direct_version: &str,
    transitive: Option<(&str, &str)>,
) -> pnpm_lockfile::Lockfile {
    use pnpm_lockfile::{
        ComVer, ImporterDepVersion, Lockfile, LockfileVersion, PackageMetadata, PkgName,
        PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, RegistryResolution, ResolvedDependencySpec,
        SnapshotDepRef, SnapshotEntry,
    };

    let direct_name = PkgName::parse(direct_name).expect("parse direct package name");
    let direct_version = direct_version.parse::<PkgVerPeer>().expect("parse direct version");
    let direct_key = PkgNameVerPeer::new(direct_name.clone(), direct_version.clone());
    let importers = importer_ids
        .iter()
        .map(|importer_id| {
            let dependencies = std::collections::HashMap::from([(
                direct_name.clone(),
                ResolvedDependencySpec {
                    specifier: direct_specifier.to_string(),
                    version: ImporterDepVersion::Regular(direct_version.clone()),
                },
            )]);
            (
                (*importer_id).to_string(),
                ProjectSnapshot { dependencies: Some(dependencies), ..ProjectSnapshot::default() },
            )
        })
        .collect();
    let metadata = || {
        PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
                .parse()
                .expect("parse integrity"),
            revision: None,
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
    };
    let mut packages = std::collections::HashMap::from([(direct_key.clone(), metadata())]);
    let mut snapshots =
        std::collections::HashMap::from([(direct_key.clone(), SnapshotEntry::default())]);
    if let Some((child_name, child_version)) = transitive {
        let child_name = PkgName::parse(child_name).expect("parse child package name");
        let child_version = child_version.parse::<PkgVerPeer>().expect("parse child version");
        let child_key = PkgNameVerPeer::new(child_name.clone(), child_version.clone());
        packages.insert(child_key.clone(), metadata());
        snapshots.insert(child_key, SnapshotEntry::default());
        snapshots.insert(
            direct_key,
            SnapshotEntry {
                dependencies: Some(std::collections::HashMap::from([(
                    child_name,
                    SnapshotDepRef::Plain(child_version),
                )])),
                ..SnapshotEntry::default()
            },
        );
    }
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

pub(super) async fn resolve_importer_scoped_update_direct(
    order: [&str; 2],
    selected_scope: crate::UpdateReuseScope,
) -> HashMap<String, String> {
    let (_selected_tmp, selected_manifest) =
        fake_manifest(serde_json::json!({ "pkg": "^100.0.0" }));
    let (_unselected_tmp, unselected_manifest) =
        fake_manifest(serde_json::json!({ "pkg": "^100.0.0" }));
    let manifests = HashMap::from_iter([
        ("selected", &selected_manifest),
        ("unselected", &unselected_manifest),
    ]);
    let importers = order
        .iter()
        .map(|id| WorkspaceImporter { id: (*id).to_string(), manifest: manifests[id] })
        .collect::<Vec<_>>();
    let resolver = RecordingResolver {
        table: HashMap::from_iter([(
            ("pkg".to_string(), "^100.0.0".to_string()),
            fake_result(
                "pkg",
                "100.1.0",
                None,
                serde_json::json!({ "name": "pkg", "version": "100.1.0" }),
            ),
        )]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(importer_scoped_update_lockfile(
        &["selected", "unselected"],
        "pkg",
        "^100.0.0",
        "100.0.0",
        None,
    )));
    opts.update_reuse_scopes_by_importer =
        BTreeMap::from([("selected".to_string(), selected_scope)]);
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
        })
        .await
        .expect("resolve importer-scoped update");
    result
        .peers
        .direct_dependencies_by_importer
        .into_iter()
        .map(|(importer_id, dependencies)| (importer_id, dependencies["pkg"].as_str().to_string()))
        .collect()
}

pub(super) fn recorded_time(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(pkg_id, published_at)| ((*pkg_id).to_string(), (*published_at).to_string()))
        .collect()
}

pub(super) fn lockfile_recording_time(entries: &[(&str, &str)]) -> pnpm_lockfile::Lockfile {
    use pnpm_lockfile::{ComVer, Lockfile, LockfileVersion};

    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::new(),
        packages: None,
        snapshots: None,
        time: Some(recorded_time(entries)),
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// A wanted lockfile whose `packages:` map holds exactly one entry.
pub(super) fn lockfile_with_package(key: &str) -> pnpm_lockfile::Lockfile {
    use pnpm_lockfile::{
        ComVer, LockfileVersion, PackageMetadata, PkgNameVerPeer, TarballResolution,
    };
    let key: PkgNameVerPeer = key.parse().expect("parse package key");
    let metadata = PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{key}.tgz"),
            integrity: None,
            revision: None,
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
    pnpm_lockfile::Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::new(),
        packages: Some(std::collections::HashMap::from([(key, metadata)])),
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// Build a reuse-seeding lockfile from a flat description:
/// one importer with `direct` deps `(alias, specifier, version)`, a
/// package/snapshot graph of `(key, [(child_alias, child_version)])`
/// entries, and an optional `catalogs:` snapshot of
/// `(catalog, alias, specifier, version)` rows.
pub(super) fn reuse_graph_lockfile(
    importer_id: &str,
    direct: &[(&str, &str, &str)],
    graph: &[(&str, &[(&str, &str)])],
    catalogs: &[(&str, &str, &str, &str)],
) -> pnpm_lockfile::Lockfile {
    use pnpm_lockfile::{
        ComVer, ImporterDepVersion, Lockfile, LockfileVersion, PackageMetadata, PkgName,
        PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, RegistryResolution, ResolvedCatalogEntry,
        ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
    };

    let dependencies = direct
        .iter()
        .map(|(alias, specifier, version)| {
            (
                PkgName::parse(*alias).expect("parse direct alias"),
                ResolvedDependencySpec {
                    specifier: (*specifier).to_string(),
                    version: ImporterDepVersion::Regular(
                        version.parse::<PkgVerPeer>().expect("parse direct version"),
                    ),
                },
            )
        })
        .collect();
    let importers = std::collections::HashMap::from([(
        importer_id.to_string(),
        ProjectSnapshot { dependencies: Some(dependencies), ..ProjectSnapshot::default() },
    )]);
    let metadata = || {
        PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
                .parse()
                .expect("parse integrity"),
            revision: None,
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
    };
    let mut packages = std::collections::HashMap::new();
    let mut snapshots = std::collections::HashMap::new();
    for (key, children) in graph {
        let key = key.parse::<PkgNameVerPeer>().expect("parse graph key");
        packages.insert(key.clone(), metadata());
        let dependencies = (!children.is_empty()).then(|| {
            children
                .iter()
                .map(|(alias, version)| {
                    (
                        PkgName::parse(*alias).expect("parse child alias"),
                        SnapshotDepRef::Plain(
                            version.parse::<PkgVerPeer>().expect("parse child version"),
                        ),
                    )
                })
                .collect()
        });
        snapshots.insert(key, SnapshotEntry { dependencies, ..SnapshotEntry::default() });
    }
    let catalog_snapshots = (!catalogs.is_empty()).then(|| {
        let mut snapshot: pnpm_lockfile::CatalogSnapshots = BTreeMap::new();
        for (catalog, alias, specifier, version) in catalogs {
            snapshot.entry((*catalog).to_string()).or_default().insert(
                (*alias).to_string(),
                ResolvedCatalogEntry {
                    specifier: (*specifier).to_string(),
                    version: (*version).to_string(),
                },
            );
        }
        snapshot
    });
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: catalog_snapshots,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

pub(super) fn graph_versions_of(
    result: &super::super::ResolveWorkspaceResult,
    name: &str,
) -> Vec<String> {
    let prefix = format!("{name}@");
    let mut versions: Vec<String> = result
        .peers
        .graph
        .keys()
        .filter_map(|dep_path| dep_path.as_str().strip_prefix(&prefix))
        .map(str::to_string)
        .collect();
    versions.sort();
    versions
}

/// `slow` is the `(alias, range)` the resolver holds back, which
/// decides whether the pinned subtree or the fresh edge records
/// `shared`'s children first.
pub(super) async fn resolve_pinned_versus_fresh(slow: (&str, &str)) -> crate::ResolvedTree {
    let dependencies = |deps| {
        move |name: &str, version: &str| {
            fake_result(
                name,
                version,
                None,
                serde_json::json!({ "name": name, "version": version, "dependencies": deps }),
            )
        }
    };
    let table = HashMap::from_iter([
        (
            ("reused".to_string(), "1.0.0".to_string()),
            dependencies(serde_json::json!({ "shared": "1.0.0" }))("reused", "1.0.0"),
        ),
        (
            ("fresh".to_string(), "1.0.0".to_string()),
            dependencies(serde_json::json!({ "shared": "^1.0.0" }))("fresh", "1.0.0"),
        ),
        (
            ("shared".to_string(), "1.0.0".to_string()),
            dependencies(serde_json::json!({ "pin": "^1.0.0" }))("shared", "1.0.0"),
        ),
        (
            ("shared".to_string(), "^1.0.0".to_string()),
            dependencies(serde_json::json!({ "pin": "^1.0.0" }))("shared", "1.0.0"),
        ),
        (
            ("pin".to_string(), "1.0.0".to_string()),
            fake_result(
                "pin",
                "1.0.0",
                None,
                serde_json::json!({ "name": "pin", "version": "1.0.0" }),
            ),
        ),
        (
            ("pin".to_string(), "^1.0.0".to_string()),
            fake_result(
                "pin",
                "1.5.0",
                None,
                serde_json::json!({ "name": "pin", "version": "1.5.0" }),
            ),
        ),
    ]);
    let resolver = SlowAliasResolver { table, slow: (slow.0.to_string(), slow.1.to_string()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "reused": "1.0.0", "fresh": "1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(Arc::new(reuse_graph_lockfile(
        ".",
        &[("reused", "1.0.0", "1.0.0")],
        &[
            ("reused@1.0.0", &[("shared", "1.0.0")]),
            ("shared@1.0.0", &[("pin", "1.0.0")]),
            ("pin@1.0.0", &[]),
        ],
        &[],
    )));
    let dir = tmp.path().to_path_buf();
    resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        importer_opts(dir.clone(), None)
    })
    .await
    .expect("resolve the pinned-versus-fresh contest")
    .merged_tree
}

pub(super) fn reuse_steal_lockfile() -> pnpm_lockfile::Lockfile {
    use pnpm_lockfile::{
        ComVer, ImporterDepVersion, Lockfile, LockfileVersion, PackageMetadata, PkgName,
        PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, RegistryResolution, ResolvedDependencySpec,
        SnapshotDepRef, SnapshotEntry,
    };

    let metadata = || {
        PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
                .parse()
                .expect("parse integrity"),
            revision: None,
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
    };
    let key = |raw: &str| PkgNameVerPeer::from_str(raw).expect("parse snapshot key");
    let importers = std::collections::HashMap::from([(
        "pkg-b".to_string(),
        ProjectSnapshot {
            dependencies: Some(std::collections::HashMap::from([(
                PkgName::parse("wrapperB").unwrap(),
                ResolvedDependencySpec {
                    specifier: "1.0.0".to_string(),
                    version: ImporterDepVersion::Regular("1.0.0".parse::<PkgVerPeer>().unwrap()),
                },
            )])),
            ..ProjectSnapshot::default()
        },
    )]);
    let packages = std::collections::HashMap::from([
        (key("wrapperB@1.0.0"), metadata()),
        (key("mid2@1.0.0"), metadata()),
        (key("leaf2@1.0.0"), metadata()),
    ]);
    let snapshots = std::collections::HashMap::from([
        (
            key("wrapperB@1.0.0"),
            SnapshotEntry {
                dependencies: Some(std::collections::HashMap::from([(
                    PkgName::parse("mid2").unwrap(),
                    SnapshotDepRef::Plain("1.0.0".parse::<PkgVerPeer>().unwrap()),
                )])),
                ..SnapshotEntry::default()
            },
        ),
        (
            key("mid2@1.0.0"),
            SnapshotEntry {
                dependencies: Some(std::collections::HashMap::from([(
                    PkgName::parse("leaf2").unwrap(),
                    SnapshotDepRef::Plain("1.0.0".parse::<PkgVerPeer>().unwrap()),
                )])),
                ..SnapshotEntry::default()
            },
        ),
        (key("leaf2@1.0.0"), SnapshotEntry::default()),
    ]);
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}
