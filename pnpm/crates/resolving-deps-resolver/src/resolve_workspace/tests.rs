//! `resolutionMode: time-based` cutoff tests for
//! [`fn@super::resolve_workspace`].

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::{DateTime, TimeZone, Utc};
use pnpm_lockfile::{DirectoryResolution, LockfileResolution, RegistryContext};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    LatestQuery, NoMatchingVersionError, PkgResolutionId, PreferredVersions, RegistryResponseError,
    RegistryResponseErrorOptions, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use pretty_assertions::assert_eq;

use super::{WorkspaceImporter, WorkspaceResolveOptions, resolve_workspace};
use crate::{
    resolve_importer::ResolveImporterOptions,
    tests::{RecordedReadPackageCalls, RecordingHooks},
};

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
    /// The `shared` specifier this resolver claims. Only a named
    /// `workspace:` selector is eligible for the cross-importer cache, so the
    /// tests vary it to cover both sides of that gate.
    shared_specifier: &'static str,
    workspace_resolutions: AtomicUsize,
}

impl ProjectRelativeWorkspaceResolver {
    fn new(target_dir: std::path::PathBuf) -> Self {
        Self::claiming("workspace:^", target_dir)
    }

    fn claiming(shared_specifier: &'static str, target_dir: std::path::PathBuf) -> Self {
        Self { target_dir, shared_specifier, workspace_resolutions: AtomicUsize::new(0) }
    }

    fn workspace_resolution_count(&self) -> usize {
        self.workspace_resolutions.load(Ordering::Relaxed)
    }
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
        let shared_specifier = self.shared_specifier;
        Box::pin(async move {
            if alias == "wrapper" && range == "1.0.0" {
                return Ok(Some(fake_result(
                    "wrapper",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "wrapper",
                        "version": "1.0.0",
                        "dependencies": { "shared": shared_specifier },
                    }),
                )));
            }
            if alias != "shared" || range != shared_specifier {
                return Ok(None);
            }
            self.workspace_resolutions.fetch_add(1, Ordering::Relaxed);
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
    use pnpm_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
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
            revision: None,
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
        dedupe_peer_dependents: true,
        all_preferred_versions: Arc::new(PreferredVersions::new()),
        override_bare_specifier: None,
        patched_dependencies: None,
        base_opts: ResolveOptions { published_by, project_dir, ..ResolveOptions::default() },
        pick_lowest_direct: false,
        subdep_published_by: published_by,
        catalogs: pnpm_catalogs_types::Catalogs::new(),
        exclude_links_from_lockfile: false,
        lockfile_dir: None,
        modules_dir: None,
        peers_suffix_max_length: 1000,
        catalog_server: false,
        manifest_hook: None,
        overrides_hook: None,
        pnpmfile_hook: None,
    }
}

fn workspace_opts(pick_lowest_direct: bool, time_based: bool) -> WorkspaceResolveOptions {
    WorkspaceResolveOptions {
        registry_context: RegistryContext::default(),
        dedupe_peers: false,
        dedupe_injected_deps: false,
        dedupe_peer_dependents: false,
        resolve_peers_from_workspace_root: false,
        exclude_links_from_lockfile: false,
        lockfile_dir: std::path::PathBuf::from("/lockfile-dir"),
        peers_suffix_max_length: 1000,
        share_workspace_resolutions: true,
        manifest_hook: None,
        overrides_hook: None,
        pnpmfile_hook: None,
        read_package_log: None,
        skipped_optional_log: None,
        finalized_package: None,
        allowed_deprecated_versions: BTreeMap::new(),
        deprecation_log: None,
        pick_lowest_direct,
        time_based,
        wanted_lockfile: None,
        reuse_lockfile_subtrees: true,
        update_reuse_scope: crate::UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        update_depth: crate::UpdateDepth::UNLIMITED,
        auto_install_peers: false,
    }
}

fn importer_scoped_update_lockfile(
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

async fn resolve_importer_scoped_update_direct(
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

#[tokio::test]
async fn importer_scoped_update_drop_only_is_order_independent() {
    for order in [["selected", "unselected"], ["unselected", "selected"]] {
        let direct = resolve_importer_scoped_update_direct(
            order,
            crate::UpdateReuseScope::Except(std::iter::once(("pkg".to_string(), None)).collect()),
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
        let manifests = HashMap::from_iter([
            ("selected", &selected_manifest),
            ("unselected", &unselected_manifest),
        ]);
        let importers = order
            .iter()
            .map(|id| WorkspaceImporter { id: (*id).to_string(), manifest: manifests[id] })
            .collect::<Vec<_>>();
        let resolver = RecordingResolver {
            table: HashMap::from_iter([
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
            seen: Mutex::new(HashMap::default()),
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
            crate::UpdateReuseScope::Except(std::iter::once(("pkg".to_string(), None)).collect()),
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
        assert_eq!(&*parent_children[0].pkg_id, "pkg@100.1.0");
        // Recording the winner's children is not enough on its own: the
        // occurrence that ran first realized the ones it resolved, and
        // only the handover makes it re-read them.
        assert_eq!(graph_versions_of(&result, "pkg"), ["100.1.0"], "order {order:?}");
    }
}

#[tokio::test]
async fn workspace_resolution_is_shared_and_rendered_per_importer() {
    let (_a_tmp, a_manifest) = fake_manifest(serde_json::json!({ "shared": "workspace:^" }));
    let (_b_tmp, b_manifest) = fake_manifest(serde_json::json!({ "shared": "workspace:^" }));
    let (_c_tmp, c_manifest) = fake_manifest(serde_json::json!({ "shared": "workspace:^" }));
    let resolver =
        ProjectRelativeWorkspaceResolver::new(std::path::PathBuf::from("/repo/packages/shared"));
    let importers = vec![
        WorkspaceImporter { id: "packages/a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "apps/b".to_string(), manifest: &b_manifest },
        WorkspaceImporter { id: "packages/c".to_string(), manifest: &c_manifest },
    ];
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let workspace_packages = std::sync::Arc::new(std::collections::BTreeMap::default());
    let hook_calls: RecordedReadPackageCalls = Arc::new(Mutex::new(Vec::new()));
    let mut opts = workspace_opts(false, false);
    opts.lockfile_dir.clone_from(&lockfile_dir);
    opts.pnpmfile_hook = Some(Arc::new(RecordingHooks { calls: Arc::clone(&hook_calls) }));

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "packages/a" => std::path::PathBuf::from("/repo/packages/a"),
                "apps/b" => std::path::PathBuf::from("/repo/apps/b"),
                "packages/c" => std::path::PathBuf::from("/repo/packages/c"),
                _ => unreachable!("unexpected importer"),
            };
            let mut opts = importer_opts(project_dir, None);
            opts.lockfile_dir = Some(lockfile_dir.clone());
            opts.base_opts.lockfile_dir.clone_from(&lockfile_dir);
            opts.base_opts.always_try_workspace_packages = true;
            opts.base_opts.workspace_packages = Some(std::sync::Arc::clone(&workspace_packages));
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
    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/c"]["shared"].as_str(),
        "link:../shared",
    );
    assert_eq!(resolver.workspace_resolution_count(), 1);
    let mut shared_hook_dirs = hook_calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "shared")
        .map(|(_, dir)| dir.clone())
        .collect::<Vec<_>>();
    shared_hook_dirs.sort();
    assert_eq!(
        shared_hook_dirs,
        [Some("../../packages/shared".to_string()), Some("../shared".to_string())],
    );
}

#[tokio::test]
async fn semver_workspace_matches_stay_scoped_to_each_importer() {
    // `always_try_workspace_packages` lets a plain semver range land on a
    // workspace package too, but only a named `workspace:` selector is
    // guaranteed to depend on the importer solely through the rendered link.
    // A range keeps its per-importer resolution.
    let (_a_tmp, a_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let (_b_tmp, b_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let resolver = ProjectRelativeWorkspaceResolver::claiming(
        "^1.0.0",
        std::path::PathBuf::from("/repo/packages/shared"),
    );
    let importers = vec![
        WorkspaceImporter { id: "packages/a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "apps/b".to_string(), manifest: &b_manifest },
    ];
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let workspace_packages = std::sync::Arc::new(std::collections::BTreeMap::default());
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
            opts.base_opts.workspace_packages = Some(std::sync::Arc::clone(&workspace_packages));
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
    assert_eq!(resolver.workspace_resolution_count(), 2);
}

#[tokio::test]
async fn canonical_snapshot_link_keeps_direct_links_relative_to_each_importer() {
    let (_nested_tmp, nested_manifest) = fake_manifest(serde_json::json!({
        "shared": "workspace:^",
        "wrapper": "1.0.0",
    }));
    let (_shallow_tmp, shallow_manifest) = fake_manifest(serde_json::json!({
        "shared": "workspace:^",
        "wrapper": "1.0.0",
    }));
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let workspace_packages = std::sync::Arc::new(std::collections::BTreeMap::default());
    let resolver = ProjectRelativeWorkspaceResolver::new(lockfile_dir.join("packages/shared"));
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
            opts.base_opts.workspace_packages = Some(std::sync::Arc::clone(&workspace_packages));
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
    assert_eq!(wrapper.children.get("shared"), Some(&crate::DepPath::from("link:packages/shared")));
    assert!(result.merged_tree.packages.contains_key("link:packages/shared"));
    assert!(!result.merged_tree.packages.contains_key("link:../../../packages/shared"));
    assert_eq!(resolver.workspace_resolution_count(), 1);
}

#[tokio::test]
async fn catalogs_work_in_injected_workspace_packages() {
    let (_project1_tmp, project1) = fake_manifest(serde_json::json!({ "project2": "workspace:*" }));
    let (_project2_tmp, project2) = fake_manifest(serde_json::json!({ "is-positive": "catalog:" }));
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
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
        seen: Mutex::new(HashMap::default()),
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
    assert_eq!(&*children[0].pkg_id, "is-positive@1.0.0");
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
    let mut table = HashMap::default();
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
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
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
    let mut table = HashMap::default();
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
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0", "b": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    let result = resolve_workspace(
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
    assert_eq!(
        result.time,
        recorded_time(&[
            ("a@1.0.0", "2024-03-01T10:00:00.000Z"),
            ("b@1.0.0", "2024-05-20T08:00:00.000Z"),
        ]),
        "the direct deps' publish dates are handed to the lockfile's `time:` section",
    );
}

/// Against a registry whose abbreviated metadata omits publish times,
/// the dates the lockfile already recorded are what the cutoff is
/// derived from — otherwise a re-resolve would compute a different
/// cutoff and could pick different subdependency versions.
#[tokio::test]
async fn time_based_cutoff_falls_back_to_the_lockfiles_recorded_time() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            None,
            serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "sub": "^2.0.0" } }),
        ),
    );
    table.insert(
        ("sub".to_string(), "^2.0.0".to_string()),
        fake_result("sub", "2.0.0", None, serde_json::json!({ "name": "sub", "version": "2.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    let mut opts = workspace_opts(true, true);
    opts.wanted_lockfile =
        Some(Arc::new(lockfile_recording_time(&[("a@1.0.0", "2024-05-20T08:00:00.000Z")])));

    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        importer_opts(tmp.path().to_path_buf(), None)
    })
    .await
    .unwrap();

    assert_eq!(
        resolver.opts_for("sub"),
        (false, Some(Utc.with_ymd_and_hms(2024, 5, 20, 9, 0, 0).unwrap())),
        "the recorded publish date stands in for the missing one",
    );
    assert_eq!(
        result.time,
        recorded_time(&[("a@1.0.0", "2024-05-20T08:00:00.000Z")]),
        "a recorded date is carried forward, not dropped",
    );
}

fn recorded_time(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(pkg_id, published_at)| ((*pkg_id).to_string(), (*published_at).to_string()))
        .collect()
}

fn lockfile_recording_time(entries: &[(&str, &str)]) -> pnpm_lockfile::Lockfile {
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

#[tokio::test]
async fn time_based_cutoff_is_clamped_by_minimum_release_age() {
    let maximum = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut table = HashMap::default();
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
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
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
    let mut table = HashMap::default();
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
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
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
    let mut table = HashMap::default();
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
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
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
    opts.wanted_lockfile = Some(std::sync::Arc::new(pnpm_lockfile::Lockfile {
        lockfile_version: pnpm_lockfile::LockfileVersion::<9>::try_from(
            pnpm_lockfile::ComVer::new(9, 0),
        )
        .unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::new(),
        packages: None,
        snapshots: Some(std::collections::HashMap::from([(
            pnpm_lockfile::PkgNameVerPeer::from_str("shared@1.0.0(opt@25.0.0)").unwrap(),
            pnpm_lockfile::SnapshotEntry::default(),
        )])),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }));
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

    let a_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a importer");
    assert_eq!(
        a_direct.get("shared").map(std::string::ToString::to_string),
        Some("shared@1.0.0(opt@25.0.0)".to_string()),
        "a locked peer provider must remain eligible for importer-local hoisting",
    );
}

#[tokio::test]
async fn shared_subtree_owner_context_is_available_before_optional_hoisting() {
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("wrapper".to_string(), "1.0.0".to_string()),
                fake_result(
                    "wrapper",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "wrapper",
                        "version": "1.0.0",
                        "dependencies": { "shared": "1.0.0" },
                    }),
                ),
            ),
            (
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
            ),
            (
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
            ),
            (
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
            ),
            (
                ("opt".to_string(), "18.0.0".to_string()),
                fake_result(
                    "opt",
                    "18.0.0",
                    None,
                    serde_json::json!({ "name": "opt", "version": "18.0.0" }),
                ),
            ),
            (
                ("opt".to_string(), "25.0.0".to_string()),
                fake_result(
                    "opt",
                    "25.0.0",
                    None,
                    serde_json::json!({ "name": "opt", "version": "25.0.0" }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let (tmp_nested, nested_manifest) =
        fake_manifest(serde_json::json!({ "wrapper": "1.0.0", "carrier": "1.0.0" }));
    let (tmp_owner, owner_manifest) =
        fake_manifest(serde_json::json!({ "shared": "1.0.0", "opt": "18.0.0" }));
    let importers = [
        WorkspaceImporter { id: "nested".to_string(), manifest: &nested_manifest },
        WorkspaceImporter { id: "owner".to_string(), manifest: &owner_manifest },
    ];
    let dirs = [tmp_nested.path(), tmp_owner.path()];
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

    let nested =
        result.peers.direct_dependencies_by_importer.get("nested").expect("nested importer");
    assert_eq!(
        nested.get("wrapper").map(std::string::ToString::to_string),
        Some("wrapper@1.0.0".to_string()),
        "the importer visited before the shared-subtree owner must not hoist the owner's peer",
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
    let mut table = HashMap::default();
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
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
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

#[tokio::test]
async fn local_workspace_package_version_can_satisfy_another_importers_optional_peer() {
    let mut table = HashMap::default();
    table.insert(
        ("needs-opt".to_string(), "1.0.0".to_string()),
        fake_result(
            "needs-opt",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "needs-opt",
                "version": "1.0.0",
                "peerDependencies": { "opt": "^1.0.0" },
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    let mut local_opt =
        fake_result("opt", "1.0.0", None, serde_json::json!({ "name": "opt", "version": "1.0.0" }));
    local_opt.id = PkgResolutionId::from("link:packages/opt".to_string());
    local_opt.name_ver = None;
    local_opt.resolution = LockfileResolution::Directory(DirectoryResolution {
        directory: "packages/opt".to_string(),
    });
    local_opt.resolved_via = "local-filesystem".to_string();
    table.insert(("opt".to_string(), "workspace:*".to_string()), local_opt);
    table.insert(
        ("opt".to_string(), "1.0.0".to_string()),
        fake_result("opt", "1.0.0", None, serde_json::json!({ "name": "opt", "version": "1.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "opt": "workspace:*" }));
    let (tmp_a, a_manifest) = fake_manifest(serde_json::json!({ "needs-opt": "1.0.0" }));
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

    let direct = result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a");
    assert_eq!(
        direct.get("needs-opt").map(std::string::ToString::to_string),
        Some("needs-opt@1.0.0(opt@1.0.0)".to_string()),
    );
    assert_eq!(
        direct.get("opt").map(std::string::ToString::to_string),
        Some("opt@1.0.0".to_string()),
    );
}

/// The pin satisfies `host`'s optional peer range and the owner's
/// pick does not, so this picker installs nothing only if unreachable
/// candidates are filtered out.
#[tokio::test]
async fn transiently_walked_subtree_versions_do_not_bias_optional_peer_hoists() {
    let host_manifest = serde_json::json!({
        "name": "host",
        "version": "1.0.0",
        "peerDependencies": { "dep": "^1.0.0" },
        "peerDependenciesMeta": { "dep": { "optional": true } },
    });
    let result = resolve_with_transient_shared_walk(
        host_manifest,
        "1.0.0",
        &[("^1.0.0", "2.0.0"), ("1.0.0", "1.0.0")],
    )
    .await;

    let direct = result.peers.direct_dependencies_by_importer.get(".").expect("root importer");
    assert_eq!(
        direct.get("host").map(std::string::ToString::to_string),
        Some("host@1.0.0".to_string()),
        "the unreachable dep@1.0.0 must not satisfy host's optional peer",
    );
    assert_eq!(direct.get("dep"), None, "no optional-peer hoist may install dep");
    assert_eq!(graph_versions_of(&result, "dep"), ["2.0.0"]);
}

/// The required-peer picker dedupes onto the highest satisfying
/// candidate, so the pin is higher than (and as satisfying as) the
/// owner's pick — the arrangement where unreachable-candidate
/// filtering is observable through this picker.
#[tokio::test]
async fn transiently_walked_subtree_versions_do_not_bias_required_peer_hoists() {
    let host_manifest = serde_json::json!({
        "name": "host",
        "version": "1.0.0",
        "peerDependencies": { "dep": "^1.0.0" },
    });
    let result = resolve_with_transient_shared_walk(
        host_manifest,
        "1.9.0",
        &[("^1.0.0", "1.5.0"), ("1.5.0", "1.5.0"), ("1.9.0", "1.9.0")],
    )
    .await;

    let direct = result.peers.direct_dependencies_by_importer.get(".").expect("root importer");
    assert_eq!(
        direct.get("host").map(std::string::ToString::to_string),
        Some("host@1.0.0(dep@1.5.0)".to_string()),
        "the required-peer hoist must dedupe onto the reachable version",
    );
    assert_eq!(
        direct.get("dep").map(std::string::ToString::to_string),
        Some("dep@1.5.0".to_string()),
    );
    assert_eq!(graph_versions_of(&result, "dep"), ["1.5.0"]);
}

/// Drive [`fn@resolve_workspace`] through the adversarial interleaving
/// of <https://github.com/pnpm/pnpm/issues/13567>.
///
/// Both importers depend on `shared@1.0.0`, whose children context is
/// deterministically owned by the root importer (same depth, lower
/// importer order). Delaying the root's `shared` resolution lets
/// `pkg-b` reuse its locked `shared` subtree first — a transient walk
/// that resolves the pinned `dep@<pinned_dep_version>` before losing
/// the children context to the root, whose fresh walk resolves
/// `dep@^1.0.0` to whatever `dep_results` maps it to. The pinned
/// version is then unreachable, so the peer-hoist pickers deciding
/// `host`'s missing `dep` peer must never see it; each caller asserts
/// that for its peer shape.
async fn resolve_with_transient_shared_walk(
    host_manifest: serde_json::Value,
    pinned_dep_version: &str,
    dep_results: &[(&str, &str)],
) -> super::ResolveWorkspaceResult {
    let (_tmp_root, root_manifest) =
        fake_manifest(serde_json::json!({ "shared": "1.0.0", "host": "1.0.0" }));
    let (_tmp_b, b_manifest) = fake_manifest(serde_json::json!({ "shared": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-b".to_string(), manifest: &b_manifest },
    ];
    let mut table = HashMap::from_iter([
        (
            ("shared".to_string(), "1.0.0".to_string()),
            fake_result(
                "shared",
                "1.0.0",
                None,
                serde_json::json!({
                    "name": "shared",
                    "version": "1.0.0",
                    "dependencies": { "dep": "^1.0.0" },
                }),
            ),
        ),
        (
            ("host".to_string(), "1.0.0".to_string()),
            fake_result("host", "1.0.0", None, host_manifest),
        ),
    ]);
    for (wanted, version) in dep_results {
        table.insert(
            ("dep".to_string(), (*wanted).to_string()),
            fake_result(
                "dep",
                version,
                None,
                serde_json::json!({ "name": "dep", "version": version }),
            ),
        );
    }
    let resolver = SlowAliasResolver { table, slow: ("shared".to_string(), "1.0.0".to_string()) };
    let pinned_dep_key = format!("dep@{pinned_dep_version}");
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.wanted_lockfile = Some(Arc::new(reuse_graph_lockfile(
        "pkg-b",
        &[("shared", "1.0.0", "1.0.0")],
        &[("shared@1.0.0", &[("dep", pinned_dep_version)]), (&pinned_dep_key, &[])],
        &[],
    )));
    resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
        importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
    })
    .await
    .expect("resolve workspace under the adversarial interleaving")
}

/// [`RecordingResolver`] with one artificially slow entry: its resolve
/// yields back to the executor enough times for every other concurrent
/// walk to run to completion first.
struct SlowAliasResolver {
    table: HashMap<(String, String), ResolveResult>,
    slow: (String, String),
}

impl Resolver for SlowAliasResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let alias = wanted.alias.clone().unwrap_or_default();
        let range = wanted.bare_specifier.clone().unwrap_or_default();
        let result = self.table.get(&(alias.clone(), range.clone())).cloned();
        let slow = self.slow == (alias, range);
        Box::pin(async move {
            if slow {
                for _ in 0..64 {
                    tokio::task::yield_now().await;
                }
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

/// Resolver fed from a `(alias, range)` → `ResolveResult` table whose
/// chain fails for the aliases in `failing`, mimicking a registry
/// whose packument no longer serves any satisfying version.
struct FailingAliasResolver {
    table: HashMap<(String, String), ResolveResult>,
    failing: HashSet<String>,
    failure: FailureShape,
}

/// How [`FailingAliasResolver`] fails. The tree walker recovers the coded
/// shapes by downcasting the type-erased [`ResolveError`], so an optional
/// dependency has to keep being skipped for each of them, not only for the
/// plain string error every other resolver produces.
#[derive(Clone, Copy)]
enum FailureShape {
    Plain,
    NoMatchingVersion,
    RegistryResponse,
}

impl FailureShape {
    fn error(self, alias: &str, range: &str) -> ResolveError {
        match self {
            FailureShape::Plain => format!("No matching version found for {alias}@{range}").into(),
            FailureShape::NoMatchingVersion => Box::new(NoMatchingVersionError {
                dep: format!("{alias}@{range}"),
                registry: "https://registry.example/".to_string(),
                published_versions: format!(r#"The latest release of {alias} is "2.0.0"."#),
            }),
            FailureShape::RegistryResponse => {
                Box::new(RegistryResponseError::new(RegistryResponseErrorOptions {
                    url: &format!("https://registry.example/{alias}"),
                    status: 404,
                    status_text: "Not Found",
                    pkg_name: alias,
                    auth_header_value: None,
                }))
            }
        }
    }
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
        let failure = self.failure;
        let result = self.table.get(&(alias.clone(), range.clone())).cloned();
        Box::pin(async move {
            if failing {
                return Err(failure.error(&alias, &range));
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
fn optional_failure_fixture(
    failure: FailureShape,
) -> (tempfile::TempDir, PackageManifest, FailingAliasResolver) {
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
        table: HashMap::from_iter([(
            ("kept".to_string(), "^1.0.0".to_string()),
            fake_result(
                "kept",
                "1.0.0",
                None,
                serde_json::json!({ "name": "kept", "version": "1.0.0" }),
            ),
        )]),
        failing: std::collections::HashSet::from_iter(["broken".to_string()]),
        failure,
    };
    (tmp, manifest, resolver)
}

/// A wanted lockfile whose `packages:` map holds exactly one entry.
fn lockfile_with_package(key: &str) -> pnpm_lockfile::Lockfile {
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

#[tokio::test]
async fn skips_an_optional_dependency_whose_resolution_fails_with_no_locked_entry() {
    let (_tmp, manifest, resolver) = optional_failure_fixture(FailureShape::Plain);
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
    let (_tmp, manifest, resolver) = optional_failure_fixture(FailureShape::Plain);
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
    let (_tmp, manifest, resolver) = optional_failure_fixture(FailureShape::Plain);
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

/// The coded resolver failures arrive as their own error variants rather
/// than the generic `Resolve` envelope, so each has to stay in the
/// optional-dependency skip arm: an optional dependency the registry has
/// no version of — or no package for — must keep dropping its edge
/// instead of failing the install.
#[tokio::test]
async fn skips_an_optional_dependency_for_every_coded_resolver_failure() {
    for failure in [FailureShape::NoMatchingVersion, FailureShape::RegistryResponse] {
        let (_tmp, manifest, resolver) = optional_failure_fixture(failure);
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
        .expect("a coded resolution failure of an optional dependency is skipped");

        let direct = &result.peers.direct_dependencies_by_importer["."];
        assert!(direct.contains_key("kept"), "the regular dep resolves: {direct:?}");
        assert!(!direct.contains_key("broken"), "the failing optional edge is dropped: {direct:?}");
        let skipped = skipped.lock().unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name.as_deref(), Some("broken"));
    }
}

/// The loud-failure path for a locked optional dependency has to cover
/// the coded failures too, for the same reason as the skip arm.
#[tokio::test]
async fn fails_loudly_on_a_locked_optional_dependency_for_every_coded_resolver_failure() {
    for failure in [FailureShape::NoMatchingVersion, FailureShape::RegistryResponse] {
        let (_tmp, manifest, resolver) = optional_failure_fixture(failure);
        let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
        let mut opts = workspace_opts(false, false);
        opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile_with_package("broken@1.2.0")));
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
        assert!(
            matches!(err, crate::ResolveDependencyTreeError::LockedOptionalResolutionFailure(_)),
            "unexpected error: {err}",
        );
    }
}

/// A newly-resolved package whose manifest carries `deprecated` is
/// forwarded to the deprecation sink once, and an
/// `allowedDeprecatedVersions` entry satisfied by the resolved version
/// suppresses the notification.
#[tokio::test]
async fn deprecated_manifests_notify_the_deprecation_sink_unless_allowed() {
    for (allowed_range, expect_notification) in
        [(None, true), (Some("^1.0.0"), false), (Some("^2.0.0"), true)]
    {
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "old": "^1.0.0" }));
        let importers = [WorkspaceImporter { id: "root".to_string(), manifest: &manifest }];
        let resolver = RecordingResolver {
            table: HashMap::from_iter([(
                ("old".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "old",
                    "1.2.0",
                    None,
                    serde_json::json!({
                        "name": "old",
                        "version": "1.2.0",
                        "deprecated": "use new instead",
                    }),
                ),
            )]),
            seen: Mutex::new(HashMap::default()),
        };
        let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&notifications);
        let mut opts = workspace_opts(false, false);
        if let Some(range) = allowed_range {
            opts.allowed_deprecated_versions =
                BTreeMap::from([("old".to_string(), range.to_string())]);
        }
        opts.deprecation_log = Some(std::sync::Arc::new(move |deprecation: crate::Deprecation| {
            sink.lock().unwrap().push(deprecation);
        }));
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
        })
        .await
        .expect("resolve workspace with a deprecated dependency");

        let notifications = notifications.lock().unwrap();
        if expect_notification {
            let [deprecation] = notifications.as_slice() else {
                panic!("expected one deprecation for range {allowed_range:?}: {notifications:?}");
            };
            assert_eq!(deprecation.pkg_name, "old");
            assert_eq!(deprecation.pkg_version, "1.2.0");
            assert_eq!(deprecation.deprecated, "use new instead");
            assert_eq!(deprecation.depth, 0);
            assert_eq!(
                deprecation.prefix,
                std::path::PathBuf::from("/repo").join("root").display().to_string(),
            );
        } else {
            assert!(
                notifications.is_empty(),
                "range {allowed_range:?} must suppress the warning: {notifications:?}",
            );
        }
    }
}

#[tokio::test]
async fn deprecated_package_is_reported_only_on_its_first_occurrence() {
    let (_transitive_tmp, transitive_manifest) =
        fake_manifest(serde_json::json!({ "wrapper": "1.0.0" }));
    let (_direct_tmp, direct_manifest) = fake_manifest(serde_json::json!({ "old": "1.0.0" }));
    // Ids chosen so the transitive importer is walked first under the
    // resolver's id-ordered importer processing.
    let importers = [
        WorkspaceImporter { id: "a-transitive".to_string(), manifest: &transitive_manifest },
        WorkspaceImporter { id: "b-direct".to_string(), manifest: &direct_manifest },
    ];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("wrapper".to_string(), "1.0.0".to_string()),
                fake_result(
                    "wrapper",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "wrapper",
                        "version": "1.0.0",
                        "dependencies": { "old": "1.0.0" },
                    }),
                ),
            ),
            (
                ("old".to_string(), "1.0.0".to_string()),
                fake_result(
                    "old",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "old",
                        "version": "1.0.0",
                        "deprecated": "use new instead",
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&notifications);
    let mut opts = workspace_opts(false, false);
    opts.deprecation_log = Some(std::sync::Arc::new(move |deprecation: crate::Deprecation| {
        sink.lock().unwrap().push(deprecation);
    }));

    resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
        importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
    })
    .await
    .expect("resolve a deprecated package reached at two depths");

    let notifications = notifications.lock().unwrap();
    let [deprecation] = notifications.as_slice() else {
        panic!("expected one deprecation from the first occurrence: {notifications:?}");
    };
    assert_eq!(deprecation.depth, 1);
    assert_eq!(
        deprecation.prefix,
        std::path::PathBuf::from("/repo").join("a-transitive").display().to_string(),
    );
}

/// The listing order of importers is not part of the input: processing
/// is id-ordered, so a reversed listing attributes the deprecation to
/// the same first occurrence (pnpm/pnpm#13846).
#[tokio::test]
async fn deprecation_attribution_does_not_depend_on_importer_listing_order() {
    let deprecation_prefix_for = |reversed: bool| async move {
        let (_transitive_tmp, transitive_manifest) =
            fake_manifest(serde_json::json!({ "wrapper": "1.0.0" }));
        let (_direct_tmp, direct_manifest) = fake_manifest(serde_json::json!({ "old": "1.0.0" }));
        let mut importers = vec![
            WorkspaceImporter { id: "a-transitive".to_string(), manifest: &transitive_manifest },
            WorkspaceImporter { id: "b-direct".to_string(), manifest: &direct_manifest },
        ];
        if reversed {
            importers.reverse();
        }
        let resolver = RecordingResolver {
            table: HashMap::from_iter([
                (
                    ("wrapper".to_string(), "1.0.0".to_string()),
                    fake_result(
                        "wrapper",
                        "1.0.0",
                        None,
                        serde_json::json!({
                            "name": "wrapper",
                            "version": "1.0.0",
                            "dependencies": { "old": "1.0.0" },
                        }),
                    ),
                ),
                (
                    ("old".to_string(), "1.0.0".to_string()),
                    fake_result(
                        "old",
                        "1.0.0",
                        None,
                        serde_json::json!({
                            "name": "old",
                            "version": "1.0.0",
                            "deprecated": "use new instead",
                        }),
                    ),
                ),
            ]),
            seen: Mutex::new(HashMap::default()),
        };
        let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&notifications);
        let mut opts = workspace_opts(false, false);
        opts.deprecation_log = Some(std::sync::Arc::new(move |deprecation: crate::Deprecation| {
            sink.lock().unwrap().push(deprecation);
        }));
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
        })
        .await
        .expect("resolve the workspace");
        let notifications = notifications.lock().unwrap();
        let [deprecation] = notifications.as_slice() else {
            panic!("expected one deprecation: {notifications:?}");
        };
        deprecation.prefix.clone()
    };

    let listed = deprecation_prefix_for(false).await;
    let reversed = deprecation_prefix_for(true).await;
    assert_eq!(listed, reversed, "attribution must not depend on the listing order");
}

/// A dependency reused from the wanted lockfile still notifies the
/// deprecation sink: the synthesized manifest round-trips the
/// lockfile's `deprecated` metadata precisely so warm installs keep
/// warning, matching pnpm's repeat-install behavior.
#[tokio::test]
async fn reused_lockfile_entries_still_notify_the_deprecation_sink() {
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "old": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "root".to_string(), manifest: &manifest }];
    let resolver =
        RecordingResolver { table: HashMap::default(), seen: Mutex::new(HashMap::default()) };
    let mut lockfile = importer_scoped_update_lockfile(&["root"], "old", "^1.0.0", "1.2.0", None);
    lockfile
        .packages
        .as_mut()
        .expect("lockfile carries packages")
        .get_mut(&"old@1.2.0".parse::<pnpm_lockfile::PkgNameVerPeer>().expect("parse key"))
        .expect("direct entry")
        .deprecated = Some("use new instead".to_string());
    let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&notifications);
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile));
    opts.deprecation_log = Some(std::sync::Arc::new(move |deprecation: crate::Deprecation| {
        sink.lock().unwrap().push(deprecation);
    }));
    resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
        importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
    })
    .await
    .expect("resolve workspace reusing the lockfile");

    let notifications = notifications.lock().unwrap();
    let [deprecation] = notifications.as_slice() else {
        panic!("expected one deprecation from the reused entry: {notifications:?}");
    };
    assert_eq!(deprecation.pkg_name, "old");
    assert_eq!(deprecation.pkg_version, "1.2.0");
    assert_eq!(deprecation.deprecated, "use new instead");
    assert_eq!(deprecation.depth, 0);
}

/// Build a reuse-seeding lockfile from a flat description:
/// one importer with `direct` deps `(alias, specifier, version)`, a
/// package/snapshot graph of `(key, [(child_alias, child_version)])`
/// entries, and an optional `catalogs:` snapshot of
/// `(catalog, alias, specifier, version)` rows.
fn reuse_graph_lockfile(
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

fn graph_versions_of(result: &super::ResolveWorkspaceResult, name: &str) -> Vec<String> {
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

/// A `catalog:` direct dep whose catalog entry is unchanged must not be
/// treated as a changed direct dep: the wanted spec reaches the walker
/// with the protocol already resolved to the catalog's range, and
/// comparing that against the recorded `catalog:` specifier would
/// decline subtree reuse for every dependent — `parent` would
/// re-resolve to the registry's newer 1.1.0 (and re-pick its open
/// `tool: *` edge at 2.0.0) with no manifest change, churning the
/// lockfile.
#[tokio::test]
async fn unchanged_catalog_dep_keeps_dependent_subtree_pins() {
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "tool": "catalog:", "parent": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "proj".to_string(), manifest: &manifest }];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("tool".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "tool",
                    "1.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "1.0.0" }),
                ),
            ),
            (
                ("tool".to_string(), "*".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("parent".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "parent",
                    "1.1.0",
                    None,
                    serde_json::json!({
                        "name": "parent",
                        "version": "1.1.0",
                        "dependencies": { "tool": "*" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_graph_lockfile(
        "proj",
        &[("tool", "catalog:", "1.0.0"), ("parent", "^1.0.0", "1.0.0")],
        &[("tool@1.0.0", &[]), ("parent@1.0.0", &[("tool", "1.0.0")])],
        &[("default", "tool", "^1.0.0", "1.0.0")],
    )));
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            opts.catalogs = BTreeMap::from([(
                "default".to_string(),
                BTreeMap::from([("tool".to_string(), "^1.0.0".to_string())]),
            )]);
            opts
        })
        .await
        .expect("resolve workspace with an unchanged catalog dep");

    assert_eq!(graph_versions_of(&result, "parent"), ["1.0.0"]);
    assert_eq!(graph_versions_of(&result, "tool"), ["1.0.0"]);
}

/// A catalog range bump is a real direct-dep change: dependents that
/// pin the old version must re-resolve so their pins land on the new
/// catalog pick.
#[tokio::test]
async fn catalog_range_bump_refreshes_dependent_pins() {
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "tool": "catalog:", "parent": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "proj".to_string(), manifest: &manifest }];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("tool".to_string(), "^2.0.0".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("tool".to_string(), "*".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("tool".to_string(), "2.0.0".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("parent".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "parent",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "parent",
                        "version": "1.0.0",
                        "dependencies": { "tool": "*" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_graph_lockfile(
        "proj",
        &[("tool", "catalog:", "1.0.0"), ("parent", "^1.0.0", "1.0.0")],
        &[("tool@1.0.0", &[]), ("parent@1.0.0", &[("tool", "1.0.0")])],
        &[("default", "tool", "^1.0.0", "1.0.0")],
    )));
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            opts.catalogs = BTreeMap::from([(
                "default".to_string(),
                BTreeMap::from([("tool".to_string(), "^2.0.0".to_string())]),
            )]);
            opts
        })
        .await
        .expect("resolve workspace with a bumped catalog range");

    assert_eq!(graph_versions_of(&result, "tool"), ["2.0.0"]);
}

/// A parent whose subtree contains a dependency cycle re-resolves
/// fresh (the conservative cycle guard), but when it lands back on its
/// recorded version its child edges keep their prior refs and re-enter
/// the reuse gate — so `stable`'s cycle-free subtree is reused and its
/// open `open: *` edge keeps the recorded 1.0.0 pin instead of
/// re-picking the registry's newest 2.0.0.
#[tokio::test]
async fn fresh_resolved_parent_on_recorded_version_reuses_child_subtrees() {
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "app": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "proj".to_string(), manifest: &manifest }];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("app".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "app",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "app",
                        "version": "1.0.0",
                        "dependencies": { "cyclic": "^1.0.0", "stable": "^1.0.0" },
                    }),
                ),
            ),
            // The cycle-membered subtree is denied reuse, so its edges
            // resolve freshly with their recorded versions pinned as
            // exact specs — the table serves the pinned form.
            (
                ("cyclic".to_string(), "1.0.0".to_string()),
                fake_result(
                    "cyclic",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "cyclic",
                        "version": "1.0.0",
                        "dependencies": { "loop": "^1.0.0" },
                    }),
                ),
            ),
            (
                ("loop".to_string(), "1.0.0".to_string()),
                fake_result(
                    "loop",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "loop",
                        "version": "1.0.0",
                        "dependencies": { "cyclic": "^1.0.0" },
                    }),
                ),
            ),
            (
                ("stable".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "stable",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "stable",
                        "version": "1.0.0",
                        "dependencies": { "open": "*" },
                    }),
                ),
            ),
            (
                ("open".to_string(), "*".to_string()),
                fake_result(
                    "open",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "open", "version": "2.0.0" }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_graph_lockfile(
        "proj",
        &[("app", "^1.0.0", "1.0.0")],
        &[
            ("app@1.0.0", &[("cyclic", "1.0.0"), ("stable", "1.0.0")]),
            ("cyclic@1.0.0", &[("loop", "1.0.0")]),
            ("loop@1.0.0", &[("cyclic", "1.0.0")]),
            ("stable@1.0.0", &[("open", "1.0.0")]),
            ("open@1.0.0", &[]),
        ],
        &[],
    )));
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
        })
        .await
        .expect("resolve workspace with a cycle next to a reusable subtree");

    for name in ["app", "cyclic", "loop", "stable", "open"] {
        assert_eq!(graph_versions_of(&result, name), ["1.0.0"], "{name} must keep 1.0.0");
    }
}

/// The root's `react` wins even though it doesn't satisfy `lucide-react`'s
/// declared peer range: keeping one copy across the workspace is the point
/// of the setting.
#[tokio::test]
async fn non_root_importer_hoists_the_root_importers_peer_provider() {
    let (_root_tmp, root_manifest) = fake_manifest(serde_json::json!({ "react": "19.2.0" }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "lucide": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("react".to_string(), "19.2.0".to_string()),
                fake_result(
                    "react",
                    "19.2.0",
                    None,
                    serde_json::json!({ "name": "react", "version": "19.2.0" }),
                ),
            ),
            (
                ("react".to_string(), "^18.0.0".to_string()),
                fake_result(
                    "react",
                    "18.3.1",
                    None,
                    serde_json::json!({ "name": "react", "version": "18.3.1" }),
                ),
            ),
            (
                ("lucide".to_string(), "1.0.0".to_string()),
                fake_result(
                    "lucide",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "lucide",
                        "version": "1.0.0",
                        "peerDependencies": { "react": "^18.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut importer_opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a root-provided peer");

    assert_eq!(graph_versions_of(&result, "react"), ["19.2.0"], "one react, the root's");
    let app_deps = &result.peers.direct_dependencies_by_importer["app-b"];
    assert_eq!(app_deps["react"].as_str(), "react@19.2.0");
    assert_eq!(app_deps["lucide"].as_str(), "lucide@1.0.0(react@19.2.0)");
}

/// A tarball / git / local root dep leaves `name_ver` unset, and the alias
/// it was declared under need not be its name.
#[tokio::test]
async fn root_dep_named_only_by_its_manifest_still_provides_the_peer() {
    const TARBALL: &str = "https://tarballs.example/real-peer-1.0.0.tgz";
    let (_root_tmp, root_manifest) = fake_manifest(serde_json::json!({ "aliased": TARBALL }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "consumer": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let mut unnamed = fake_result(
        "real-peer",
        "1.0.0",
        None,
        serde_json::json!({ "name": "real-peer", "version": "1.0.0" }),
    );
    unnamed.name_ver = None;
    unnamed.id = pnpm_resolving_resolver_base::PkgResolutionId::from(TARBALL.to_string());
    unnamed.alias = Some("aliased".to_string());
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (("aliased".to_string(), TARBALL.to_string()), unnamed.clone()),
            (("real-peer".to_string(), TARBALL.to_string()), unnamed),
            (
                ("real-peer".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "real-peer",
                    "1.9.9",
                    None,
                    serde_json::json!({ "name": "real-peer", "version": "1.9.9" }),
                ),
            ),
            (
                ("consumer".to_string(), "1.0.0".to_string()),
                fake_result(
                    "consumer",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "consumer",
                        "version": "1.0.0",
                        "peerDependencies": { "real-peer": "^1.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut importer_opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a manifest-named root peer provider");

    assert!(
        !result.peers.graph.keys().any(|dep_path| dep_path.as_str().contains("1.9.9")),
        "the peer must come from the root's tarball dep, not a second copy off the registry: {:?}",
        result.peers.graph.keys().map(|k| k.as_str().to_string()).collect::<Vec<_>>(),
    );
}

/// A `file:` tarball has no directory to read a version out of, so the
/// root's specifier reaches no candidate at all. Both shapes a local
/// resolution can take are covered: with a manifest the dep is nameable
/// and carries the resolver's own specifier, without one it is named by
/// its alias from the declared specifier.
#[tokio::test]
async fn a_project_relative_root_dep_is_not_offered_as_a_peer_provider() {
    project_relative_root_dep_is_not_a_provider(
        "file:./vendor/real-peer.tgz",
        ManifestAvailability::Absent,
    )
    .await;
}

#[tokio::test]
async fn a_nameable_project_relative_root_dep_is_not_offered_as_a_peer_provider() {
    project_relative_root_dep_is_not_a_provider(
        "file:./vendor/real-peer.tgz",
        ManifestAvailability::Present,
    )
    .await;
}

/// The path form of `workspace:` names a directory relative to the
/// declaring project, so it goes through the same manifest read as
/// `link:` and `file:` — unlike the range form, which
/// [`fn@a_workspace_range_root_dep_is_offered_as_a_peer_provider`] covers.
/// Nothing is on disk at the path here, so it yields no candidate.
#[tokio::test]
async fn a_workspace_path_root_dep_is_not_offered_as_a_peer_provider() {
    project_relative_root_dep_is_not_a_provider(
        "workspace:../packages/real-peer",
        ManifestAvailability::Present,
    )
    .await;
}

#[derive(Clone, Copy)]
enum ManifestAvailability {
    Present,
    Absent,
}

async fn project_relative_root_dep_is_not_a_provider(local: &str, manifest: ManifestAvailability) {
    let (_root_tmp, root_manifest) = fake_manifest(serde_json::json!({ "real-peer": local }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "consumer": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let mut unnamed = fake_result(
        "real-peer",
        "1.0.0",
        None,
        serde_json::json!({ "name": "real-peer", "version": "1.0.0" }),
    );
    unnamed.name_ver = None;
    if matches!(manifest, ManifestAvailability::Absent) {
        unnamed.manifest = None;
    }
    unnamed.normalized_bare_specifier = Some(local.to_string());
    unnamed.id = pnpm_resolving_resolver_base::PkgResolutionId::from(local.to_string());
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (("real-peer".to_string(), local.to_string()), unnamed),
            (
                ("real-peer".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "real-peer",
                    "1.9.9",
                    None,
                    serde_json::json!({ "name": "real-peer", "version": "1.9.9" }),
                ),
            ),
            (
                ("consumer".to_string(), "1.0.0".to_string()),
                fake_result(
                    "consumer",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "consumer",
                        "version": "1.0.0",
                        "peerDependencies": { "real-peer": "^1.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut importer_opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a project-relative root dep");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["app-b"]["real-peer"].as_str(),
        "real-peer@1.9.9",
        "`{local}` must not be hoisted into app-b",
    );
}

/// The root's authority over the peer does not depend on the protocol it
/// declared the package with: the linked package's own version stands in
/// for its path, so a sibling's newer copy loses to it exactly as it would
/// to a registry dependency of the root.
#[tokio::test]
async fn a_link_root_dep_provides_the_peer_at_the_linked_packages_version() {
    link_root_dep_peer_provider(Some("1.2.3"), "real-peer@1.2.3").await;
}

/// Nothing stands in for the path when the target names no version, so the
/// root offers no candidate and the peer falls through to the graph — the
/// path itself must never survive as the specifier.
#[tokio::test]
async fn a_versionless_link_root_dep_is_not_offered_as_a_peer_provider() {
    link_root_dep_peer_provider(None, "real-peer@1.9.9").await;
}

async fn link_root_dep_peer_provider(linked_version: Option<&str>, expected: &str) {
    let root_tmp = tempfile::tempdir().expect("tempdir");
    let linked_dir = root_tmp.path().join("vendor/real-peer");
    std::fs::create_dir_all(&linked_dir).expect("create the linked package's directory");
    let linked_manifest = match linked_version {
        Some(version) => serde_json::json!({ "name": "real-peer", "version": version }),
        None => serde_json::json!({ "name": "real-peer" }),
    };
    std::fs::write(linked_dir.join("package.json"), linked_manifest.to_string())
        .expect("write the linked package's manifest");
    let root_manifest_path = root_tmp.path().join("package.json");
    std::fs::write(
        &root_manifest_path,
        serde_json::json!({
            "name": "root",
            "version": "0.0.0",
            "dependencies": { "real-peer": "link:vendor/real-peer" },
        })
        .to_string(),
    )
    .expect("write the root manifest");
    let root_manifest =
        PackageManifest::from_path(root_manifest_path).expect("parse root manifest");
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "consumer": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let mut linked = fake_result(
        "real-peer",
        "1.2.3",
        None,
        serde_json::json!({ "name": "real-peer", "version": "1.2.3" }),
    );
    linked.name_ver = None;
    linked.normalized_bare_specifier = Some("link:vendor/real-peer".to_string());
    linked.id = pnpm_resolving_resolver_base::PkgResolutionId::from("link:vendor/real-peer");
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (("real-peer".to_string(), "link:vendor/real-peer".to_string()), linked),
            (
                ("real-peer".to_string(), "1.2.3".to_string()),
                fake_result(
                    "real-peer",
                    "1.2.3",
                    None,
                    serde_json::json!({ "name": "real-peer", "version": "1.2.3" }),
                ),
            ),
            (
                ("real-peer".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "real-peer",
                    "1.9.9",
                    None,
                    serde_json::json!({ "name": "real-peer", "version": "1.9.9" }),
                ),
            ),
            (
                ("consumer".to_string(), "1.0.0".to_string()),
                fake_result(
                    "consumer",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "consumer",
                        "version": "1.0.0",
                        "peerDependencies": { "real-peer": "^1.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let root_dir = root_tmp.path().to_path_buf();
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir =
                if importer.id == "." { root_dir.clone() } else { root_dir.join(&importer.id) };
            let mut importer_opts = importer_opts(project_dir, None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a link: root dep");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["app-b"]["real-peer"].as_str(),
        expected,
    );
}

/// A `workspace:` range selects the same workspace package from every
/// importer, so the root's copy satisfies another importer's peer instead
/// of a second copy off the registry.
#[tokio::test]
async fn a_workspace_range_root_dep_is_offered_as_a_peer_provider() {
    const WORKSPACE_RANGE: &str = "workspace:^1.0.0";
    const LINK: &str = "link:../packages/real-peer";
    let (_root_tmp, root_manifest) =
        fake_manifest(serde_json::json!({ "real-peer": WORKSPACE_RANGE }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "consumer": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let mut linked = fake_result(
        "real-peer",
        "1.0.0",
        None,
        serde_json::json!({ "name": "real-peer", "version": "1.0.0" }),
    );
    linked.name_ver = None;
    linked.normalized_bare_specifier = Some(WORKSPACE_RANGE.to_string());
    linked.id = pnpm_resolving_resolver_base::PkgResolutionId::from(LINK.to_string());
    linked.resolution = LockfileResolution::Directory(DirectoryResolution {
        directory: "../packages/real-peer".to_string(),
    });
    linked.resolved_via = "workspace".to_string();
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (("real-peer".to_string(), WORKSPACE_RANGE.to_string()), linked),
            (
                ("real-peer".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "real-peer",
                    "1.9.9",
                    None,
                    serde_json::json!({ "name": "real-peer", "version": "1.9.9" }),
                ),
            ),
            (
                ("consumer".to_string(), "1.0.0".to_string()),
                fake_result(
                    "consumer",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "consumer",
                        "version": "1.0.0",
                        "peerDependencies": { "real-peer": "^1.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut importer_opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a workspace: range root dep");

    assert_eq!(result.peers.direct_dependencies_by_importer["."]["real-peer"].as_str(), LINK);
    assert_eq!(
        result.peers.direct_dependencies_by_importer["app-b"]["real-peer"].as_str(),
        "link:../../packages/real-peer",
        "app-b's peer is the same workspace package, reached from app-b's own directory",
    );
    assert!(
        !result.peers.graph.keys().any(|dep_path| dep_path.as_str().contains("1.9.9")),
        "no second copy off the registry: {:?}",
        result.peers.graph.keys().map(|key| key.as_str().to_string()).collect::<Vec<_>>(),
    );
}

/// End-to-end companion of
/// `resolve_peers::tests::cached_subtree_reuse_reports_no_peer_providers`:
/// pkg-b reuses two foreign-owned subtrees — `mid`, whose walk resolved
/// `peerpkg`, and `s2wrap`, whose consumer's miss the owning importer
/// satisfied from its own ancestors (hiding it from pkg-b's hoist).
#[tokio::test]
async fn importer_sharing_foreign_subtrees_binds_peers_from_workspace_root() {
    let mut table = HashMap::default();
    table.insert(
        ("mid".to_string(), "1.0.0".to_string()),
        fake_result(
            "mid",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "mid",
                "version": "1.0.0",
                "dependencies": { "peerpkg": "2.0.0", "consumer": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("consumer".to_string(), "1.0.0".to_string()),
        fake_result(
            "consumer",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "consumer",
                "version": "1.0.0",
                "peerDependencies": { "peerpkg": "*", "peerx": "*" },
            }),
        ),
    );
    table.insert(
        ("holder".to_string(), "1.0.0".to_string()),
        fake_result(
            "holder",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "holder",
                "version": "1.0.0",
                "dependencies": { "peerpkg": "2.0.0", "s2wrap": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("s2wrap".to_string(), "1.0.0".to_string()),
        fake_result(
            "s2wrap",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "s2wrap",
                "version": "1.0.0",
                "dependencies": { "consumer2": "1.0.0" },
            }),
        ),
    );
    // pkg-b reaches `s2wrap` through `bwrap` so both importers see it at
    // depth 1 and the children-owner claim falls to pkg-a2 (lower
    // importer order) — a direct depth-0 reference would make pkg-b the
    // owner and no cross-importer sharing would occur.
    table.insert(
        ("bwrap".to_string(), "1.0.0".to_string()),
        fake_result(
            "bwrap",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "bwrap",
                "version": "1.0.0",
                "dependencies": { "s2wrap": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("consumer2".to_string(), "1.0.0".to_string()),
        fake_result(
            "consumer2",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "consumer2",
                "version": "1.0.0",
                "peerDependencies": { "peerpkg": "*" },
            }),
        ),
    );
    for version in ["1.0.0", "2.0.0"] {
        table.insert(
            ("peerpkg".to_string(), version.to_string()),
            fake_result(
                "peerpkg",
                version,
                None,
                serde_json::json!({ "name": "peerpkg", "version": version }),
            ),
        );
    }
    // `peerx` keeps `mid`'s subtree non-pure: `consumer` resolves it
    // against the importer's own direct dep, so the subtree's verdict
    // enters the peers cache (pure subtrees bypass it) and pkg-b's
    // revisit exercises the cache-replay path under test.
    table.insert(
        ("peerx".to_string(), "1.0.0".to_string()),
        fake_result(
            "peerx",
            "1.0.0",
            None,
            serde_json::json!({ "name": "peerx", "version": "1.0.0" }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "peerpkg": "1.0.0" }));
    let (tmp_a, a_manifest) =
        fake_manifest(serde_json::json!({ "mid": "1.0.0", "peerx": "1.0.0" }));
    let (tmp_a2, a2_manifest) = fake_manifest(serde_json::json!({ "holder": "1.0.0" }));
    let (tmp_b, b_manifest) =
        fake_manifest(serde_json::json!({ "mid": "1.0.0", "bwrap": "1.0.0", "peerx": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "pkg-a2".to_string(), manifest: &a2_manifest },
        WorkspaceImporter { id: "pkg-b".to_string(), manifest: &b_manifest },
    ];
    let dirs = [tmp_root.path(), tmp_a.path(), tmp_a2.path(), tmp_b.path()];

    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let mut next = 0;
    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.auto_install_peers = true;
        opts.resolve_peers_from_workspace_root = true;
        opts
    })
    .await
    .unwrap();

    // Under pkg-a2, `consumer2` binds to holder's peerpkg@2.0.0.
    let a2_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a2").expect("pkg-a2 importer");
    assert_eq!(
        a2_direct.get("holder").map(std::string::ToString::to_string),
        Some("holder@1.0.0".to_string()),
        "holder satisfies its subtree's peer internally",
    );
    let a_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a importer");
    assert_eq!(
        a_direct.get("mid").map(std::string::ToString::to_string),
        Some("mid@1.0.0(peerx@1.0.0)".to_string()),
        "consumer's peerx resolves against pkg-a's direct dep",
    );

    // Under pkg-b, `consumer2` has no provider in its own tree: its peer
    // must fall back to the workspace root's peerpkg@1.0.0, not bind to
    // the peerpkg@2.0.0 provider a reused subtree's walk resolved.
    let b_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-b").expect("pkg-b importer");
    assert_eq!(
        b_direct.get("bwrap").map(std::string::ToString::to_string),
        Some("bwrap@1.0.0(peerpkg@1.0.0)".to_string()),
        "pkg-b's own consumers must not inherit a reused subtree's provider",
    );
    assert_eq!(
        b_direct.get("mid").map(std::string::ToString::to_string),
        Some("mid@1.0.0(peerx@1.0.0)".to_string()),
    );
}

/// A children-ownership handover whose peer-shadow context is
/// unchanged must not discard other occurrences' realized subtrees.
///
/// `pkg-b`'s lockfile pins `wrapperB → mid2 → leaf2@1.0.0`, so its walk
/// reuses that subtree. The root importer's required-peer hoist of
/// `mid2` (for `needyC`) then claims `mid2`'s children ownership at
/// depth 0 with the same (empty) peer-shadow set. Rewriting the
/// displaced owner's occurrences to lazy on such a handover would
/// re-resolve the reused subtree's open ranges — churning the locked
/// `leaf2@1.0.0` to the registry's newer `1.5.0` even though nothing
/// about `mid2`'s child resolution context changed.
#[tokio::test]
async fn unchanged_shadow_ownership_handover_keeps_reused_subtree() {
    let mut table = HashMap::default();
    table.insert(
        ("wrapperB".to_string(), "1.0.0".to_string()),
        fake_result(
            "wrapperB",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "wrapperB",
                "version": "1.0.0",
                "dependencies": { "mid2": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("shared2".to_string(), "1.0.0".to_string()),
        fake_result(
            "shared2",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "shared2",
                "version": "1.0.0",
                "dependencies": { "mid2": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("needyC".to_string(), "1.0.0".to_string()),
        fake_result(
            "needyC",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "needyC",
                "version": "1.0.0",
                "peerDependencies": { "mid2": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("mid2".to_string(), "1.0.0".to_string()),
        fake_result(
            "mid2",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "mid2",
                "version": "1.0.0",
                "dependencies": { "leaf2": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("leaf2".to_string(), "^1.0.0".to_string()),
        fake_result(
            "leaf2",
            "1.5.0",
            None,
            serde_json::json!({ "name": "leaf2", "version": "1.5.0" }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_b, b_manifest) = fake_manifest(serde_json::json!({ "wrapperB": "1.0.0" }));
    // The root carries only the peer consumer: with importers walked in
    // id order the root's wave runs first, so the shared subtree must
    // be claimed by pkg-b's wave and reach the root only through the
    // later peer-hoist round for a handover to occur at all.
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "needyC": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: "pkg-b".to_string(), manifest: &b_manifest },
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
    ];
    let dirs = [tmp_b.path(), tmp_root.path()];

    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_steal_lockfile()));
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
        root_direct.get("mid2").map(std::string::ToString::to_string),
        Some("mid2@1.0.0".to_string()),
        "needyC's required peer mid2 is hoisted to the root importer",
    );
    let mid2 = result
        .peers
        .graph
        .get(&pnpm_deps_path::DepPath::from("mid2@1.0.0".to_string()))
        .expect("mid2 in graph");
    assert_eq!(
        mid2.children.get("leaf2").map(std::string::ToString::to_string),
        Some("leaf2@1.0.0".to_string()),
        "the lockfile-reused subtree must survive the ownership handover",
    );
}
/// Publishing a fresh answer over pinned children re-resolves the open
/// ranges reuse exists to hold still, and leaves the occurrences that
/// realized the pinned subtree reading children the record no longer
/// holds (<https://github.com/pnpm/pnpm/issues/13837>).
#[tokio::test]
async fn a_pinned_subtree_keeps_its_children_against_a_fresh_walk() {
    for slow in [("fresh", "1.0.0"), ("reused", "1.0.0")] {
        let tree = resolve_pinned_versus_fresh(slow).await;
        let recorded: Vec<&str> = tree
            .children_by_id
            .get("shared@1.0.0")
            .expect("shared children")
            .iter()
            .map(|edge| &*edge.pkg_id)
            .collect();
        assert_eq!(recorded, ["pin@1.0.0"], "the pins stand, held back: {slow:?}");
        assert!(
            !tree.packages.contains_key("pin@1.5.0"),
            "and nothing re-resolves the range they pinned, held back: {slow:?}",
        );
    }
}

/// `slow` is the `(alias, range)` the resolver holds back, which
/// decides whether the pinned subtree or the fresh edge records
/// `shared`'s children first.
async fn resolve_pinned_versus_fresh(slow: (&str, &str)) -> crate::ResolvedTree {
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

fn reuse_steal_lockfile() -> pnpm_lockfile::Lockfile {
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

/// Resolver that yields inside every resolution, so the runtime is free
/// to interleave whatever is in flight, and records the importers whose
/// resolutions overlapped while it did.
struct OverlapRecordingResolver {
    /// Project dirs currently inside `resolve`, and every set of two or
    /// more that were ever in flight together.
    in_flight: Mutex<HashSet<std::path::PathBuf>>,
    overlaps: Mutex<Vec<Vec<std::path::PathBuf>>>,
}

impl OverlapRecordingResolver {
    fn new() -> Self {
        OverlapRecordingResolver {
            in_flight: Mutex::new(HashSet::default()),
            overlaps: Mutex::new(Vec::new()),
        }
    }
}

impl Resolver for OverlapRecordingResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let project_dir = opts.project_dir.clone();
        let alias = wanted.alias.clone();
        Box::pin(async move {
            let Some(alias) = alias else { return Ok(None) };
            {
                let mut in_flight = self.in_flight.lock().unwrap();
                in_flight.insert(project_dir.clone());
                if in_flight.len() > 1 {
                    let mut overlapping: Vec<_> = in_flight.iter().cloned().collect();
                    overlapping.sort();
                    self.overlaps.lock().unwrap().push(overlapping);
                }
            }
            // Give every other in-flight resolution a chance to run.
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
            self.in_flight.lock().unwrap().remove(&project_dir);

            let name_ver = pnpm_lockfile::PkgNameVer::new(
                pnpm_lockfile::PkgName::parse(&alias).expect("alias parses as a package name"),
                node_semver::Version::from_str("1.0.0").expect("version parses"),
            );
            Ok::<_, ResolveError>(Some(ResolveResult {
                id: PkgResolutionId::from(&name_ver),
                name_ver: Some(name_ver),
                latest: Some("1.0.0".to_string()),
                published_at: None,
                manifest: Some(Arc::new(serde_json::json!({
                    "name": alias,
                    "version": "1.0.0",
                }))),
                resolution: LockfileResolution::Directory(DirectoryResolution {
                    directory: format!("/repo/{alias}"),
                }),
                resolved_via: "npm-registry".to_string(),
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

/// Importers' initial waves must not overlap. Interleaving them leaves
/// the resolved packages and their children identical but not the
/// occurrence nodes: importers race for a package's children-ownership
/// claim, and a transient holder still leaves its occurrences behind.
/// Occurrence identity feeds peer-variant computation, so a count that
/// depends on the interleaving is a lockfile that depends on it too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn importer_waves_do_not_overlap() {
    let (_a_tmp, a_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let (_b_tmp, b_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let (_c_tmp, c_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let resolver = OverlapRecordingResolver::new();
    let importers = vec![
        WorkspaceImporter { id: "packages/a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "packages/b".to_string(), manifest: &b_manifest },
        WorkspaceImporter { id: "packages/c".to_string(), manifest: &c_manifest },
    ];

    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(false, false),
        |importer| importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None),
    )
    .await
    .expect("resolve workspace");

    let overlaps = resolver.overlaps.lock().unwrap();
    assert!(
        overlaps.is_empty(),
        "importers resolved concurrently: {:?}",
        overlaps.first().expect("checked non-empty"),
    );
}

/// Package ids announced as finalized, each with its child ids.
type Announcements = Vec<(String, Vec<String>)>;

/// Resolve `manifest_deps` through `table` and return every package the
/// walk announced as finalized, in announcement order, with the child
/// ids each announcement carried.
async fn announced_finalized_packages(
    manifest_deps: serde_json::Value,
    table: HashMap<(String, String), ResolveResult>,
) -> Announcements {
    let (_tmp, manifest) = fake_manifest(manifest_deps);
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let importers = vec![WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    let announced: Arc<Mutex<Announcements>> = Arc::new(Mutex::new(Vec::new()));
    let mut opts = workspace_opts(false, false);
    let sink = Arc::clone(&announced);
    opts.finalized_package = Some(Arc::new(move |package| {
        let children =
            package.children.iter().map(|child| child.pkg_id.to_string()).collect::<Vec<_>>();
        sink.lock().unwrap().push((package.pkg_id.to_string(), children));
    }));
    resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        importer_opts(std::path::PathBuf::from("/repo"), None)
    })
    .await
    .expect("resolve");
    let announced = announced.lock().unwrap();
    announced.clone()
}

fn table_entry(name: &str, manifest: serde_json::Value) -> ((String, String), ResolveResult) {
    ((name.to_string(), "^1.0.0".to_string()), fake_result(name, "1.0.0", None, manifest))
}

#[tokio::test]
async fn finalized_packages_are_announced_once_their_peer_free_subtree_settles() {
    let announced = announced_finalized_packages(
        serde_json::json!({ "pure": "^1.0.0", "peered": "^1.0.0" }),
        HashMap::from_iter([
            table_entry("pure", serde_json::json!({ "dependencies": { "leaf": "^1.0.0" } })),
            table_entry("leaf", serde_json::json!({})),
            table_entry(
                "peered",
                serde_json::json!({
                    "dependencies": { "leaf": "^1.0.0" },
                    "peerDependencies": { "react": "*" },
                    "peerDependenciesMeta": { "react": { "optional": true } },
                }),
            ),
        ]),
    )
    .await;
    // `peered` declares a peer, so neither it nor its dep path is final;
    // `pure` and `leaf` are, and `pure` is announced with its edge.
    assert_eq!(
        announced,
        vec![
            ("leaf@1.0.0".to_string(), vec![]),
            ("pure@1.0.0".to_string(), vec!["leaf@1.0.0".to_string()]),
        ],
    );
}

#[tokio::test]
async fn finalized_packages_include_peer_free_cycles() {
    let announced = announced_finalized_packages(
        serde_json::json!({ "ping": "^1.0.0" }),
        HashMap::from_iter([
            table_entry("ping", serde_json::json!({ "dependencies": { "pong": "^1.0.0" } })),
            table_entry("pong", serde_json::json!({ "dependencies": { "ping": "^1.0.0" } })),
        ]),
    )
    .await;
    let ids = announced.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>();
    assert_eq!(ids, ["ping@1.0.0", "pong@1.0.0"]);
}

/// Resolver fed from an `(alias, range)` table that records every call
/// in order and can hold one alias back until another has been asked
/// for, which is how a test observes work the walk does before a level
/// barrier lifts.
struct WarmupProbeResolver {
    table: HashMap<(String, String), ResolveResult>,
    calls: Mutex<Vec<(String, String)>>,
    /// `(held, release)`: resolving `held` completes only once `release`
    /// has been requested.
    gate: Option<(String, String)>,
    released: std::sync::atomic::AtomicBool,
}

impl WarmupProbeResolver {
    fn new(table: HashMap<(String, String), ResolveResult>) -> Self {
        WarmupProbeResolver {
            table,
            calls: Mutex::new(Vec::new()),
            gate: None,
            released: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn calls_for(&self, alias: &str, range: &str) -> usize {
        self.calls.lock().unwrap().iter().filter(|(a, r)| a == alias && r == range).count()
    }
}

impl Resolver for WarmupProbeResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let alias = wanted.alias.clone().unwrap_or_default();
        let range = wanted.bare_specifier.clone().unwrap_or_default();
        self.calls.lock().unwrap().push((alias.clone(), range.clone()));
        let result = self.table.get(&(alias.clone(), range)).cloned();
        let (held, release) = self
            .gate
            .as_ref()
            .map_or((false, false), |(held, release)| (*held == alias, *release == alias));
        if release {
            self.released.store(true, std::sync::atomic::Ordering::Release);
        }
        Box::pin(async move {
            while held && !self.released.load(std::sync::atomic::Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
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

fn caret_entry(name: &str, manifest: serde_json::Value) -> ((String, String), ResolveResult) {
    ((name.to_string(), "^1.0.0".to_string()), fake_result(name, "1.0.0", None, manifest))
}

fn deps(entries: &[(&str, &str)]) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = entries
        .iter()
        .map(|(name, range)| ((*name).to_string(), serde_json::Value::from(*range)))
        .collect();
    serde_json::json!({ "dependencies": map })
}

async fn resolve_single_importer(
    resolver: &WarmupProbeResolver,
    manifest_deps: serde_json::Value,
    opts: WorkspaceResolveOptions,
    patched: Option<Arc<pnpm_patching::PatchGroupRecord>>,
) -> Result<super::ResolveWorkspaceResult, tokio::time::error::Elapsed> {
    let (_tmp, manifest) = fake_manifest(manifest_deps);
    let importers = vec![WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        resolve_workspace(resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
            let mut opts = importer_opts(std::path::PathBuf::from("/repo"), None);
            opts.patched_dependencies = patched.clone();
            opts
        }),
    )
    .await
    .map(|result| result.expect("resolve"))
}

#[tokio::test]
async fn warm_up_requests_descendants_beyond_one_level_before_the_level_barrier_lifts() {
    // `slow` is a direct dep that resolves only once the grandchild `c`
    // has been requested. The level barrier waits for `slow`, so only a
    // warm-up that recurses past `a`'s children can ever ask for `c`.
    let mut resolver = WarmupProbeResolver::new(HashMap::from_iter([
        caret_entry("slow", serde_json::json!({})),
        caret_entry("a", deps(&[("b", "^1.0.0")])),
        caret_entry("b", deps(&[("c", "^1.0.0")])),
        caret_entry("c", serde_json::json!({})),
    ]));
    resolver.gate = Some(("slow".to_string(), "c".to_string()));
    let result = resolve_single_importer(
        &resolver,
        serde_json::json!({ "slow": "^1.0.0", "a": "^1.0.0" }),
        workspace_opts(false, false),
        None,
    )
    .await
    .expect("the grandchild is warmed while the first level is still resolving");
    assert_eq!(graph_versions_of(&result, "c"), ["1.0.0"]);
}

#[tokio::test]
async fn warm_up_resolves_each_edge_once_across_a_diamond() {
    let resolver = WarmupProbeResolver::new(HashMap::from_iter([
        caret_entry("x", deps(&[("z", "^1.0.0")])),
        caret_entry("y", deps(&[("z", ">=1.0.0")])),
        caret_entry("z", deps(&[("w", "^1.0.0")])),
        (
            ("z".to_string(), ">=1.0.0".to_string()),
            fake_result("z", "1.0.0", None, deps(&[("w", "^1.0.0")])),
        ),
        caret_entry("w", serde_json::json!({})),
    ]));
    let result = resolve_single_importer(
        &resolver,
        serde_json::json!({ "x": "^1.0.0", "y": "^1.0.0" }),
        workspace_opts(false, false),
        None,
    )
    .await
    .expect("resolve");
    assert_eq!(graph_versions_of(&result, "z"), ["1.0.0"]);
    // Two ranges reach `z`, so it is asked for once per range; its own
    // child is asked for once however many branches reach `z`.
    assert_eq!(resolver.calls_for("z", "^1.0.0"), 1);
    assert_eq!(resolver.calls_for("z", ">=1.0.0"), 1);
    assert_eq!(resolver.calls_for("w", "^1.0.0"), 1);
}

#[tokio::test]
async fn warm_up_skips_dependencies_a_package_declares_as_its_own_peers() {
    // `p` lists `q` both as a dependency and as a peer. Under
    // autoInstallPeers the walk drops the dependency edge and resolves
    // the peer at the importer, so the dependency range must never be
    // asked for, not even speculatively two levels down.
    let resolver = WarmupProbeResolver::new(HashMap::from_iter([
        caret_entry("a", deps(&[("p", "^1.0.0")])),
        caret_entry(
            "p",
            serde_json::json!({
                "dependencies": { "q": "^2.0.0" },
                "peerDependencies": { "q": "^1.0.0" },
            }),
        ),
        caret_entry("q", serde_json::json!({})),
        (
            ("q".to_string(), "^2.0.0".to_string()),
            fake_result("q", "2.0.0", None, serde_json::json!({})),
        ),
    ]));
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    let result =
        resolve_single_importer(&resolver, serde_json::json!({ "a": "^1.0.0" }), opts, None)
            .await
            .expect("resolve");
    assert_eq!(graph_versions_of(&result, "q"), ["1.0.0"]);
    assert_eq!(resolver.calls_for("q", "^2.0.0"), 0);
}

#[tokio::test]
async fn warm_up_of_a_speculative_only_edge_leaves_patch_bookkeeping_alone() {
    // Without autoInstallPeers a dependency shadowed by a peer is dropped
    // only when the parent scope supplies the peer; the warm-up does not
    // know that scope and asks for `q@^2.0.0` speculatively. The real
    // walk never accepts that edge, so its patch must not count as
    // applied.
    let resolver = WarmupProbeResolver::new(HashMap::from_iter([
        caret_entry("a", deps(&[("p", "^1.0.0")])),
        caret_entry(
            "p",
            serde_json::json!({
                "dependencies": { "q": "^2.0.0" },
                "peerDependencies": { "q": "^1.0.0" },
            }),
        ),
        caret_entry("q", serde_json::json!({})),
        (
            ("q".to_string(), "^2.0.0".to_string()),
            fake_result("q", "2.0.0", None, serde_json::json!({})),
        ),
    ]));
    let mut groups = pnpm_patching::PatchGroupRecord::new();
    let mut group = pnpm_patching::PatchGroup::default();
    group.exact.insert(
        "2.0.0".to_string(),
        pnpm_patching::ExtendedPatchInfo {
            hash: "abc123".to_string(),
            patch_file_path: None,
            key: "q@2.0.0".to_string(),
        },
    );
    groups.insert("q".to_string(), group);
    let result = resolve_single_importer(
        &resolver,
        serde_json::json!({ "a": "^1.0.0", "q": "^1.0.0" }),
        workspace_opts(false, false),
        Some(Arc::new(groups)),
    )
    .await
    .expect("resolve");
    assert_eq!(resolver.calls_for("q", "^2.0.0"), 1, "the edge was warmed speculatively");
    assert_eq!(graph_versions_of(&result, "q"), ["1.0.0"], "and never entered the graph");
    assert!(
        !result.merged_tree.applied_patches.contains("q@2.0.0"),
        "a patch the real walk never applied must not count as applied",
    );
}
