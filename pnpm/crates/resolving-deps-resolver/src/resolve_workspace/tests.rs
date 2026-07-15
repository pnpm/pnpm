//! `resolutionMode: time-based` cutoff tests for
//! [`fn@super::resolve_workspace`].

use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::Mutex,
};

use chrono::{DateTime, TimeZone, Utc};
use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{
    LatestQuery, PkgResolutionId, PreferredVersions, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, WantedDependency,
};
use pretty_assertions::assert_eq;

use super::{WorkspaceImporter, WorkspaceResolveOptions, resolve_workspace};
use crate::resolve_importer::ResolveImporterOptions;

/// The `(pick_lowest_version, published_by)` pair recorded per alias.
type RecordedOpts = (bool, Option<DateTime<Utc>>);

/// Resolver fed from a `(alias, range)` → `ResolveResult` table that
/// records the [`RecordedOpts`] each alias was last resolved with.
struct RecordingResolver {
    table: HashMap<(String, String), ResolveResult>,
    seen: Mutex<HashMap<String, RecordedOpts>>,
}

impl RecordingResolver {
    fn opts_for(&self, alias: &str) -> RecordedOpts {
        *self.seen.lock().unwrap().get(alias).expect("alias was resolved")
    }
}

impl Resolver for RecordingResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let alias = wanted.alias.clone().unwrap_or_default();
        let range = wanted.bare_specifier.clone().unwrap_or_default();
        self.seen
            .lock()
            .unwrap()
            .insert(alias.clone(), (opts.pick_lowest_version, opts.published_by));
        let result = self.table.get(&(alias, range)).cloned();
        Box::pin(async move { Ok::<_, ResolveError>(result) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

struct ProjectRelativeWorkspaceResolver {
    target_dir: std::path::PathBuf,
}

impl Resolver for ProjectRelativeWorkspaceResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let alias = wanted.alias.clone().unwrap_or_default();
        let range = wanted.bare_specifier.clone().unwrap_or_default();
        let target_dir = self.target_dir.clone();
        let project_dir = opts.project_dir.clone();
        Box::pin(async move {
            if alias == "wrapper" && range == "1.0.0" {
                return Ok(Some(fake_result(
                    "wrapper",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "wrapper",
                        "version": "1.0.0",
                        "dependencies": { "shared": "^1.0.0" },
                    }),
                )));
            }
            if alias != "shared" || range != "^1.0.0" {
                return Ok(None);
            }
            let rel = pathdiff::diff_paths(&target_dir, &project_dir)
                .expect("target can be relativized")
                .display()
                .to_string()
                .replace('\\', "/");
            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(format!("link:{rel}")),
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: Some(std::sync::Arc::new(
                    serde_json::json!({ "name": "shared", "version": "1.0.0" }),
                )),
                resolution: LockfileResolution::Directory(DirectoryResolution { directory: rel }),
                resolved_via: "workspace".to_string(),
                normalized_bare_specifier: None,
                alias: Some(alias),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

fn fake_result(
    name: &str,
    version: &str,
    published_at: Option<&str>,
    manifest: serde_json::Value,
) -> ResolveResult {
    use pacquet_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
        latest: Some(version.to_string()),
        published_at: published_at.map(str::to_string),
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{name}-{version}.tgz"),
            integrity: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn fake_manifest(deps: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    let json = serde_json::json!({ "name": "root", "version": "0.0.0", "dependencies": deps });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

fn importer_opts(
    project_dir: std::path::PathBuf,
    published_by: Option<DateTime<Utc>>,
) -> ResolveImporterOptions {
    ResolveImporterOptions {
        auto_install_peers: false,
        auto_install_peers_from_highest_match: false,
        resolve_peers_from_workspace_root: false,
        dedupe_peers: false,
        all_preferred_versions: PreferredVersions::new(),
        patched_dependencies: None,
        base_opts: ResolveOptions { published_by, project_dir, ..ResolveOptions::default() },
        pick_lowest_direct: false,
        subdep_published_by: published_by,
        catalogs: pacquet_catalogs_types::Catalogs::new(),
        exclude_links_from_lockfile: false,
        lockfile_dir: None,
        modules_dir: None,
        peers_suffix_max_length: 1000,
        catalog_server: false,
        manifest_hook: None,
        pnpmfile_hook: None,
    }
}

fn workspace_opts(pick_lowest_direct: bool, time_based: bool) -> WorkspaceResolveOptions {
    WorkspaceResolveOptions {
        dedupe_peers: false,
        dedupe_injected_deps: false,
        dedupe_peer_dependents: false,
        resolve_peers_from_workspace_root: false,
        exclude_links_from_lockfile: false,
        lockfile_dir: std::path::PathBuf::from("/lockfile-dir"),
        peers_suffix_max_length: 1000,
        manifest_hook: None,
        pnpmfile_hook: None,
        read_package_log: None,
        skipped_optional_log: None,
        pick_lowest_direct,
        time_based,
        wanted_lockfile: None,
        update_reuse_scope: crate::UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        auto_install_peers: false,
        registries: HashMap::new(),
    }
}

fn importer_scoped_update_lockfile(
    importer_ids: &[&str],
    direct_name: &str,
    direct_specifier: &str,
    direct_version: &str,
    transitive: Option<(&str, &str)>,
) -> pacquet_lockfile::Lockfile {
    use pacquet_lockfile::{
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
            let dependencies = HashMap::from([(
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
    let mut packages = HashMap::from([(direct_key.clone(), metadata())]);
    let mut snapshots = HashMap::from([(direct_key.clone(), SnapshotEntry::default())]);
    if let Some((child_name, child_version)) = transitive {
        let child_name = PkgName::parse(child_name).expect("parse child package name");
        let child_version = child_version.parse::<PkgVerPeer>().expect("parse child version");
        let child_key = PkgNameVerPeer::new(child_name.clone(), child_version.clone());
        packages.insert(child_key.clone(), metadata());
        snapshots.insert(child_key, SnapshotEntry::default());
        snapshots.insert(
            direct_key,
            SnapshotEntry {
                dependencies: Some(HashMap::from([(
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
    }
}

async fn resolve_importer_scoped_update_direct(
    order: [&str; 2],
    selected_scope: crate::UpdateReuseScope,
) -> HashMap<String, String> {
    let (_selected_tmp, selected_manifest) =
        fake_manifest(serde_json::json!({ "pkg": "^100.0.0" }));
    let (_unselected_tmp, unselected_manifest) =
        fake_manifest(serde_json::json!({ "pkg": "^100.0.0" }));
    let manifests =
        HashMap::from([("selected", &selected_manifest), ("unselected", &unselected_manifest)]);
    let importers = order
        .iter()
        .map(|id| WorkspaceImporter { id: (*id).to_string(), manifest: manifests[id] })
        .collect::<Vec<_>>();
    let resolver = RecordingResolver {
        table: HashMap::from([(
            ("pkg".to_string(), "^100.0.0".to_string()),
            fake_result(
                "pkg",
                "100.1.0",
                None,
                serde_json::json!({ "name": "pkg", "version": "100.1.0" }),
            ),
        )]),
        seen: Mutex::new(HashMap::new()),
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

#[tokio::test]
async fn importer_scoped_update_drop_only_is_order_independent() {
    for order in [["selected", "unselected"], ["unselected", "selected"]] {
        let direct = resolve_importer_scoped_update_direct(
            order,
            crate::UpdateReuseScope::Except(std::collections::HashSet::from(["pkg".to_string()])),
        )
        .await;
        assert_eq!(direct["selected"], "pkg@100.1.0");
        assert_eq!(direct["unselected"], "pkg@100.0.0");
    }
}

#[tokio::test]
async fn importer_scoped_update_drop_all_is_order_independent() {
    for order in [["selected", "unselected"], ["unselected", "selected"]] {
        let direct =
            resolve_importer_scoped_update_direct(order, crate::UpdateReuseScope::None).await;
        assert_eq!(direct["selected"], "pkg@100.1.0");
        assert_eq!(direct["unselected"], "pkg@100.0.0");
    }
}

#[tokio::test]
async fn importer_scoped_update_route_owns_shared_parent_children_in_either_order() {
    for order in [["selected", "unselected"], ["unselected", "selected"]] {
        let (_selected_tmp, selected_manifest) =
            fake_manifest(serde_json::json!({ "parent": "^1.0.0" }));
        let (_unselected_tmp, unselected_manifest) =
            fake_manifest(serde_json::json!({ "parent": "^1.0.0" }));
        let manifests =
            HashMap::from([("selected", &selected_manifest), ("unselected", &unselected_manifest)]);
        let importers = order
            .iter()
            .map(|id| WorkspaceImporter { id: (*id).to_string(), manifest: manifests[id] })
            .collect::<Vec<_>>();
        let resolver = RecordingResolver {
            table: HashMap::from([
                (
                    ("parent".to_string(), "^1.0.0".to_string()),
                    fake_result(
                        "parent",
                        "1.0.0",
                        None,
                        serde_json::json!({
                            "name": "parent",
                            "version": "1.0.0",
                            "dependencies": { "pkg": "^100.0.0" },
                        }),
                    ),
                ),
                (
                    ("pkg".to_string(), "^100.0.0".to_string()),
                    fake_result(
                        "pkg",
                        "100.1.0",
                        None,
                        serde_json::json!({ "name": "pkg", "version": "100.1.0" }),
                    ),
                ),
            ]),
            seen: Mutex::new(HashMap::new()),
        };
        let mut opts = workspace_opts(false, false);
        opts.wanted_lockfile = Some(std::sync::Arc::new(importer_scoped_update_lockfile(
            &["selected", "unselected"],
            "parent",
            "^1.0.0",
            "1.0.0",
            Some(("pkg", "100.0.0")),
        )));
        opts.update_reuse_scopes_by_importer = BTreeMap::from([(
            "selected".to_string(),
            crate::UpdateReuseScope::Except(std::collections::HashSet::from(["pkg".to_string()])),
        )]);
        let result =
            resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
            })
            .await
            .expect("resolve shared parent update");

        for importer_id in ["selected", "unselected"] {
            assert_eq!(
                result.peers.direct_dependencies_by_importer[importer_id]["parent"].as_str(),
                "parent@1.0.0",
            );
        }
        let parent_children =
            result.merged_tree.children_by_id.get("parent@1.0.0").expect("parent children");
        assert_eq!(parent_children.len(), 1);
        assert_eq!(parent_children[0].pkg_id, "pkg@100.1.0");
    }
}

#[tokio::test]
async fn workspace_link_results_are_cached_per_importer_project_dir() {
    let (_a_tmp, a_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let (_b_tmp, b_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let resolver = ProjectRelativeWorkspaceResolver {
        target_dir: std::path::PathBuf::from("/repo/packages/shared"),
    };
    let importers = vec![
        WorkspaceImporter { id: "packages/a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "apps/b".to_string(), manifest: &b_manifest },
    ];
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let mut opts = workspace_opts(false, false);
    opts.lockfile_dir.clone_from(&lockfile_dir);

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "packages/a" => std::path::PathBuf::from("/repo/packages/a"),
                "apps/b" => std::path::PathBuf::from("/repo/apps/b"),
                _ => unreachable!("unexpected importer"),
            };
            let mut opts = importer_opts(project_dir, None);
            opts.lockfile_dir = Some(lockfile_dir.clone());
            opts.base_opts.lockfile_dir.clone_from(&lockfile_dir);
            opts.base_opts.always_try_workspace_packages = true;
            opts.base_opts.workspace_packages = Some(std::collections::BTreeMap::default());
            opts
        })
        .await
        .expect("resolve workspace");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/a"]["shared"].as_str(),
        "link:../shared",
    );
    assert_eq!(
        result.peers.direct_dependencies_by_importer["apps/b"]["shared"].as_str(),
        "link:../../packages/shared",
    );
}

#[tokio::test]
async fn canonical_snapshot_link_keeps_direct_links_relative_to_each_importer() {
    let (_nested_tmp, nested_manifest) = fake_manifest(serde_json::json!({
        "shared": "^1.0.0",
        "wrapper": "1.0.0",
    }));
    let (_shallow_tmp, shallow_manifest) = fake_manifest(serde_json::json!({
        "shared": "^1.0.0",
        "wrapper": "1.0.0",
    }));
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let resolver =
        ProjectRelativeWorkspaceResolver { target_dir: lockfile_dir.join("packages/shared") };
    let importers = vec![
        WorkspaceImporter { id: "apps/nested/app".to_string(), manifest: &nested_manifest },
        WorkspaceImporter { id: "packages/consumer".to_string(), manifest: &shallow_manifest },
    ];
    let mut opts = workspace_opts(false, false);
    opts.lockfile_dir.clone_from(&lockfile_dir);

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "apps/nested/app" => std::path::PathBuf::from("/repo/apps/nested/app"),
                "packages/consumer" => std::path::PathBuf::from("/repo/packages/consumer"),
                _ => unreachable!("unexpected importer"),
            };
            let mut opts = importer_opts(project_dir, None);
            opts.lockfile_dir = Some(lockfile_dir.clone());
            opts.base_opts.lockfile_dir.clone_from(&lockfile_dir);
            opts.base_opts.always_try_workspace_packages = true;
            opts.base_opts.workspace_packages = Some(std::collections::BTreeMap::default());
            opts
        })
        .await
        .expect("resolve workspace");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["apps/nested/app"]["shared"].as_str(),
        "link:../../../packages/shared",
    );
    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/consumer"]["shared"].as_str(),
        "link:../shared",
    );
    let wrapper =
        result.peers.graph.get(&crate::DepPath::from("wrapper@1.0.0")).expect("wrapper graph node");
    assert_eq!(wrapper.children.get("shared"), Some(&crate::DepPath::from("link:packages/shared")),);
    assert!(result.merged_tree.packages.contains_key("link:packages/shared"));
    assert!(!result.merged_tree.packages.contains_key("link:../../../packages/shared"));
}

#[tokio::test]
async fn catalogs_work_in_injected_workspace_packages() {
    let (_project1_tmp, project1) = fake_manifest(serde_json::json!({ "project2": "workspace:*" }));
    let (_project2_tmp, project2) = fake_manifest(serde_json::json!({ "is-positive": "catalog:" }));
    let resolver = RecordingResolver {
        table: HashMap::from([
            (
                ("project2".to_string(), "workspace:*".to_string()),
                ResolveResult {
                    id: PkgResolutionId::from("file:packages/project2".to_string()),
                    name_ver: None,
                    latest: None,
                    published_at: None,
                    manifest: Some(std::sync::Arc::new(serde_json::json!({
                        "name": "project2",
                        "version": "0.0.0",
                        "dependencies": { "is-positive": "catalog:" },
                    }))),
                    resolution: LockfileResolution::Directory(DirectoryResolution {
                        directory: "packages/project2".to_string(),
                    }),
                    resolved_via: "workspace".to_string(),
                    normalized_bare_specifier: None,
                    alias: Some("project2".to_string()),
                    policy_violation: None,
                },
            ),
            (
                ("is-positive".to_string(), "1.0.0".to_string()),
                fake_result(
                    "is-positive",
                    "1.0.0",
                    None,
                    serde_json::json!({ "name": "is-positive", "version": "1.0.0" }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::new()),
    };
    let importers = [
        WorkspaceImporter { id: "packages/project1".to_string(), manifest: &project1 },
        WorkspaceImporter { id: "packages/project2".to_string(), manifest: &project2 },
    ];
    let catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("is-positive".to_string(), "1.0.0".to_string())]),
    )]);

    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(false, false),
        |importer| {
            let mut opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            opts.catalogs = catalogs.clone();
            opts
        },
    )
    .await
    .expect("resolve catalog dependency of injected workspace package");

    assert!(result.merged_tree.packages.contains_key("project2@file:packages/project2"));
    assert!(result.merged_tree.packages.contains_key("is-positive@1.0.0"));
    let children = result
        .merged_tree
        .children_by_id
        .get("project2@file:packages/project2")
        .expect("injected workspace package children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].alias, "is-positive");
    assert_eq!(children[0].pkg_id, "is-positive@1.0.0");
    assert!(!children[0].optional);
}

#[tokio::test]
async fn workspace_root_direct_deps_resolve_child_importer_peers() {
    let (_root_tmp, root_manifest) = fake_manifest(serde_json::json!({
        "typescript": "~5.9.3",
    }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({
        "rollup": "^4.0.0",
        "plugin": "^1.0.0",
    }));
    let mut table = HashMap::new();
    table.insert(
        ("typescript".to_string(), "~5.9.3".to_string()),
        fake_result(
            "typescript",
            "5.9.3",
            None,
            serde_json::json!({ "name": "typescript", "version": "5.9.3" }),
        ),
    );
    table.insert(
        ("typescript".to_string(), "5.9.3".to_string()),
        fake_result(
            "typescript",
            "5.9.3",
            None,
            serde_json::json!({ "name": "typescript", "version": "5.9.3" }),
        ),
    );
    table.insert(
        ("rollup".to_string(), "^4.0.0".to_string()),
        fake_result(
            "rollup",
            "4.0.0",
            None,
            serde_json::json!({ "name": "rollup", "version": "4.0.0" }),
        ),
    );
    table.insert(
        ("plugin".to_string(), "^1.0.0".to_string()),
        fake_result(
            "plugin",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "plugin",
                "version": "1.0.0",
                "peerDependencies": {
                    "rollup": "^4.0.0",
                    "typescript": "^5.0.0"
                }
            }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::new()) };
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "packages/app".to_string(), manifest: &app_manifest },
    ];
    let mut opts = workspace_opts(false, false);
    opts.resolve_peers_from_workspace_root = true;

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "." => std::path::PathBuf::from("/repo"),
                "packages/app" => std::path::PathBuf::from("/repo/packages/app"),
                _ => unreachable!("unexpected importer"),
            };
            importer_opts(project_dir, None)
        })
        .await
        .expect("resolve workspace");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/app"]["plugin"].as_str(),
        "plugin@1.0.0(rollup@4.0.0)(typescript@5.9.3)",
    );
}

#[tokio::test]
async fn time_based_cutoff_is_newest_direct_publish_plus_one_hour() {
    let mut table = HashMap::new();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            Some("2024-03-01T10:00:00.000Z"),
            serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "sub": "^2.0.0" } }),
        ),
    );
    table.insert(
        ("b".to_string(), "^1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            Some("2024-05-20T08:00:00.000Z"),
            serde_json::json!({ "name": "b", "version": "1.0.0" }),
        ),
    );
    table.insert(
        ("sub".to_string(), "^2.0.0".to_string()),
        fake_result("sub", "2.0.0", None, serde_json::json!({ "name": "sub", "version": "2.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::new()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0", "b": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(true, true),
        |_| importer_opts(tmp.path().to_path_buf(), None),
    )
    .await
    .unwrap();

    let expected_cutoff = Utc.with_ymd_and_hms(2024, 5, 20, 9, 0, 0).unwrap();
    assert_eq!(
        resolver.opts_for("a"),
        (true, None),
        "direct deps pick lowest under maximum (none)",
    );
    assert_eq!(resolver.opts_for("b"), (true, None));
    assert_eq!(
        resolver.opts_for("sub"),
        (false, Some(expected_cutoff)),
        "subdep picks highest, constrained to newest-direct + 1h",
    );
}

#[tokio::test]
async fn time_based_cutoff_is_clamped_by_minimum_release_age() {
    let maximum = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut table = HashMap::new();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            Some("2024-05-20T08:00:00.000Z"),
            serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "sub": "^2.0.0" } }),
        ),
    );
    table.insert(
        ("sub".to_string(), "^2.0.0".to_string()),
        fake_result("sub", "2.0.0", None, serde_json::json!({ "name": "sub", "version": "2.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::new()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(true, true),
        |_| importer_opts(tmp.path().to_path_buf(), Some(maximum)),
    )
    .await
    .unwrap();

    assert_eq!(
        resolver.opts_for("a"),
        (true, Some(maximum)),
        "direct deps use the minimumReleaseAge cutoff",
    );
    assert_eq!(
        resolver.opts_for("sub"),
        (false, Some(maximum)),
        "the later time-based candidate is clamped to the minimumReleaseAge cutoff",
    );
}

#[tokio::test]
async fn lowest_direct_applies_no_publish_cutoff() {
    let mut table = HashMap::new();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            Some("2024-05-20T08:00:00.000Z"),
            serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "sub": "^2.0.0" } }),
        ),
    );
    table.insert(
        ("sub".to_string(), "^2.0.0".to_string()),
        fake_result("sub", "2.0.0", None, serde_json::json!({ "name": "sub", "version": "2.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::new()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(true, false),
        |_| importer_opts(tmp.path().to_path_buf(), None),
    )
    .await
    .unwrap();

    assert_eq!(resolver.opts_for("a"), (true, None));
    assert_eq!(
        resolver.opts_for("sub"),
        (false, None),
        "no time-based cutoff in lowest-direct mode",
    );
}

/// A package shared across importers keeps the children missing-peer
/// report from the importer that resolved it first, so a later importer
/// never hoists an optional peer declared inside that shared subtree.
/// The final workspace-wide peer pass still uses each importer's actual
/// provider context, so an importer without the provider gets the
/// peerless variant instead of reusing the first importer's suffixed
/// variant.
#[tokio::test]
async fn shared_subtree_owner_context_suppresses_later_optional_hoist() {
    let mut table = HashMap::new();
    table.insert(
        ("shared".to_string(), "1.0.0".to_string()),
        fake_result(
            "shared",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
                "dependencies": { "mid": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("mid".to_string(), "1.0.0".to_string()),
        fake_result(
            "mid",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "mid",
                "version": "1.0.0",
                "peerDependencies": { "opt": "*" },
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    for version in ["18.0.0", "25.0.0"] {
        table.insert(
            ("opt".to_string(), version.to_string()),
            fake_result(
                "opt",
                version,
                None,
                serde_json::json!({ "name": "opt", "version": version }),
            ),
        );
    }
    // `carrier` puts `opt@25.0.0` into the run-resolved preferred
    // versions during the root importer's walk — deep enough that it
    // is not in any peer scope — so a later hoist would pick it as the
    // max satisfying version.
    table.insert(
        ("carrier".to_string(), "1.0.0".to_string()),
        fake_result(
            "carrier",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "carrier",
                "version": "1.0.0",
                "dependencies": { "opt": "25.0.0" },
            }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::new()) };
    let (tmp_root, root_manifest) = fake_manifest(
        serde_json::json!({ "shared": "1.0.0", "opt": "18.0.0", "carrier": "1.0.0" }),
    );
    let (tmp_a, a_manifest) = fake_manifest(serde_json::json!({ "shared": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-a".to_string(), manifest: &a_manifest },
    ];
    let dirs = [tmp_root.path(), tmp_a.path()];

    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    let mut next = 0;
    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.auto_install_peers = true;
        opts
    })
    .await
    .unwrap();

    let root_direct = result.peers.direct_dependencies_by_importer.get(".").expect("root importer");
    assert_eq!(
        root_direct.get("shared").map(std::string::ToString::to_string),
        Some("shared@1.0.0(opt@18.0.0)".to_string()),
    );
    let a_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a importer");
    assert_eq!(
        a_direct.get("shared").map(std::string::ToString::to_string),
        Some("shared@1.0.0".to_string()),
        "pkg-a must not hoist opt, but it also must not reuse root's opt provider",
    );
}

/// The reverse of the sharing case above: when the first importer's
/// walk could NOT satisfy the optional peer either (it only hoisted it
/// later), the miss stays visible to every importer — each hoists its
/// own copy, so the shared subtree carries the peer suffix under both
/// (pnpm 11.6.0 behaviour for e.g. `clipanion`'s `typanion` under
/// importers that share `@yarnpkg/*` chains with the root).
#[tokio::test]
async fn shared_subtree_miss_unsatisfied_by_first_importer_still_hoists() {
    let mut table = HashMap::new();
    table.insert(
        ("top".to_string(), "1.0.0".to_string()),
        fake_result(
            "top",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "top",
                "version": "1.0.0",
                "dependencies": { "mid": "1.0.0", "carrier": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("mid".to_string(), "1.0.0".to_string()),
        fake_result(
            "mid",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "mid",
                "version": "1.0.0",
                "peerDependencies": { "opt": "*" },
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("carrier".to_string(), "1.0.0".to_string()),
        fake_result(
            "carrier",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "carrier",
                "version": "1.0.0",
                "dependencies": { "opt": "25.0.0" },
            }),
        ),
    );
    table.insert(
        ("opt".to_string(), "25.0.0".to_string()),
        fake_result(
            "opt",
            "25.0.0",
            None,
            serde_json::json!({ "name": "opt", "version": "25.0.0" }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::new()) };
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "top": "1.0.0" }));
    let (tmp_a, a_manifest) = fake_manifest(serde_json::json!({ "top": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-a".to_string(), manifest: &a_manifest },
    ];
    let dirs = [tmp_root.path(), tmp_a.path()];

    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    let mut next = 0;
    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.auto_install_peers = true;
        opts
    })
    .await
    .unwrap();

    for importer in [".", "pkg-a"] {
        let direct = result.peers.direct_dependencies_by_importer.get(importer).expect("importer");
        assert_eq!(
            direct.get("top").map(std::string::ToString::to_string),
            Some("top@1.0.0(opt@25.0.0)".to_string()),
            "{importer} hoists the peer the first walk could not satisfy",
        );
    }
}

/// Resolver fed from a `(alias, range)` → `ResolveResult` table whose
/// chain fails for the aliases in `failing`, mimicking a registry
/// whose packument no longer serves any satisfying version.
struct FailingAliasResolver {
    table: HashMap<(String, String), ResolveResult>,
    failing: std::collections::HashSet<String>,
}

impl Resolver for FailingAliasResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let alias = wanted.alias.clone().unwrap_or_default();
        let range = wanted.bare_specifier.clone().unwrap_or_default();
        let failing = self.failing.contains(&alias);
        let result = self.table.get(&(alias.clone(), range.clone())).cloned();
        Box::pin(async move {
            if failing {
                return Err(format!("No matching version found for {alias}@{range}").into());
            }
            Ok::<_, ResolveError>(result)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

/// One importer whose manifest carries a resolvable regular dep
/// (`kept`) and an optional dep (`broken`) whose resolution fails.
fn optional_failure_fixture() -> (tempfile::TempDir, PackageManifest, FailingAliasResolver) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    let json = serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": { "kept": "^1.0.0" },
        "optionalDependencies": { "broken": "^1.0.0" },
    });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    let resolver = FailingAliasResolver {
        table: HashMap::from([(
            ("kept".to_string(), "^1.0.0".to_string()),
            fake_result(
                "kept",
                "1.0.0",
                None,
                serde_json::json!({ "name": "kept", "version": "1.0.0" }),
            ),
        )]),
        failing: std::collections::HashSet::from(["broken".to_string()]),
    };
    (tmp, manifest, resolver)
}

/// A wanted lockfile whose `packages:` map holds exactly one entry.
fn lockfile_with_package(key: &str) -> pacquet_lockfile::Lockfile {
    use pacquet_lockfile::{
        ComVer, LockfileVersion, PackageMetadata, PkgNameVerPeer, TarballResolution,
    };
    let key: PkgNameVerPeer = key.parse().expect("parse package key");
    let metadata = PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{key}.tgz"),
            integrity: None,
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
    pacquet_lockfile::Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: Some(HashMap::from([(key, metadata)])),
        snapshots: None,
    }
}

#[tokio::test]
async fn skips_an_optional_dependency_whose_resolution_fails_with_no_locked_entry() {
    let (_tmp, manifest, resolver) = optional_failure_fixture();
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    let skipped = std::sync::Arc::new(Mutex::new(Vec::new()));
    let mut opts = workspace_opts(false, false);
    opts.skipped_optional_log = Some(std::sync::Arc::new({
        let skipped = std::sync::Arc::clone(&skipped);
        move |notification| skipped.lock().unwrap().push(notification)
    }));
    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod, DependencyGroup::Optional],
        opts,
        |_| importer_opts(std::path::PathBuf::from("/repo"), None),
    )
    .await
    .expect("resolution failure of an optional dependency is skipped");

    let direct = &result.peers.direct_dependencies_by_importer["."];
    assert!(direct.contains_key("kept"), "the regular dep resolves: {direct:?}");
    assert!(!direct.contains_key("broken"), "the failing optional edge is dropped: {direct:?}");
    let skipped = skipped.lock().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].name.as_deref(), Some("broken"));
    assert_eq!(skipped[0].version.as_deref(), Some("^1.0.0"));
    assert_eq!(skipped[0].bare_specifier, "^1.0.0");
    assert!(
        skipped[0].parents.is_empty(),
        "a direct optional dep has an empty parents chain: {:?}",
        skipped[0].parents,
    );
    assert_eq!(skipped[0].prefix, "/repo");
    assert!(
        skipped[0].details.contains("No matching version found for broken@^1.0.0"),
        "details carry the resolver error: {}",
        skipped[0].details,
    );
}

// Covers <https://github.com/pnpm/pnpm/issues/12853>: a wanted lockfile
// that already resolved the optional dependency must fail the install
// loudly instead of silently dropping the locked entries.
#[tokio::test]
async fn fails_on_an_optional_dependency_that_cannot_be_resolved_with_a_satisfying_locked_entry() {
    let (_tmp, manifest, resolver) = optional_failure_fixture();
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile_with_package("broken@1.2.0")));
    // Model `pacquet dedupe`: the prior lockfile rides along for the
    // locked-entry check but nothing is reused from it.
    opts.update_reuse_scope = crate::UpdateReuseScope::None;
    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod, DependencyGroup::Optional],
        opts,
        |_| importer_opts(std::path::PathBuf::from("/repo"), None),
    )
    .await;

    let Err(crate::ResolveImporterError::Resolve(err)) = result else {
        panic!("a locked optional dependency must fail loudly");
    };
    let help = miette::Diagnostic::help(&err).expect("carries the lockfile hint").to_string();
    assert!(help.contains("the lockfile contains a resolution for it"), "unexpected hint: {help}");
    assert!(
        matches!(err, crate::ResolveDependencyTreeError::LockedOptionalResolutionFailure(_)),
        "unexpected error: {err}",
    );
}

#[tokio::test]
async fn skips_an_optional_dependency_when_the_locked_entry_does_not_satisfy_the_wanted_range() {
    let (_tmp, manifest, resolver) = optional_failure_fixture();
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile_with_package("broken@0.9.0")));
    opts.update_reuse_scope = crate::UpdateReuseScope::None;
    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod, DependencyGroup::Optional],
        opts,
        |_| importer_opts(std::path::PathBuf::from("/repo"), None),
    )
    .await
    .expect("an out-of-range locked entry keeps the skip behavior");

    let direct = &result.peers.direct_dependencies_by_importer["."];
    assert!(!direct.contains_key("broken"), "the failing optional edge is dropped: {direct:?}");
}
