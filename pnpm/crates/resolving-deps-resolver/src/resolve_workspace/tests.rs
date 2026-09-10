//! `resolutionMode: time-based` cutoff tests for
//! [`fn@super::resolve_workspace`].

mod lockfile_fixtures;
use lockfile_fixtures::{
    graph_versions_of, importer_scoped_update_lockfile, lockfile_recording_time,
    lockfile_with_package, recorded_time, resolve_importer_scoped_update_direct,
    resolve_pinned_versus_fresh, reuse_graph_lockfile, reuse_steal_lockfile,
};

mod optional_dependencies;

mod behavior;

mod lockfile_reuse;

mod version_selection;

mod peer_dependencies;

mod workspace_links;

mod resolution_order;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::{DateTime, Utc};
use pnpm_lockfile::{DirectoryResolution, LockfileResolution, RegistryContext};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    LatestQuery, LinkWorkspacePackages, NoMatchingVersionError, PkgResolutionId, PreferredVersions,
    RegistryResponseError, RegistryResponseErrorOptions, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, WantedDependency,
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
