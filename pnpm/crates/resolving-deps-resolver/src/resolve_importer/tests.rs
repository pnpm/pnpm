use super::{locked_peers::importer_locked_peer_versions, missing_peers::merge_ranges};
mod workspace_links;

mod resolution_order;

mod lockfile_reuse;

mod behavior;

mod overrides;

mod optional_peers;

mod locked_peers;

use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use pnpm_lockfile::SnapshotEntry;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, LatestQuery, PreferredVersions, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, VersionSelectorEntry,
    VersionSelectorType, VersionSelectorWithWeight, VersionSelectors, WantedDependency,
};
use pretty_assertions::assert_eq;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::{
    DepPath, ResolveDependencyTreeError, resolve_importer,
    resolve_importer::{ResolveImporterError, ResolveImporterOptions},
};

/// A `packages:`/`snapshots:` pair keyed by the given depPaths, with
/// every field the peer-context tests do not read left empty.
fn peer_context_lockfile<const SNAPSHOTS: usize>(
    package: Option<(&str, pnpm_lockfile::PackageMetadata)>,
    snapshots: [(&str, pnpm_lockfile::SnapshotEntry); SNAPSHOTS],
) -> pnpm_lockfile::Lockfile {
    use pnpm_lockfile::{ComVer, Lockfile, LockfileVersion, PkgNameVerPeer};

    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::new(),
        packages: package.map(|(key, metadata)| {
            std::collections::HashMap::from([(PkgNameVerPeer::from_str(key).unwrap(), metadata)])
        }),
        snapshots: Some(
            snapshots
                .into_iter()
                .map(|(key, entry)| (PkgNameVerPeer::from_str(key).unwrap(), entry))
                .collect(),
        ),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// `packages:` metadata whose only content is the declared peer names,
/// each with a `*` range.
fn peer_declaring_metadata<const PEERS: usize>(
    peer_names: [&str; PEERS],
) -> pnpm_lockfile::PackageMetadata {
    use pnpm_lockfile::{DirectoryResolution, LockfileResolution, PackageMetadata};

    PackageMetadata {
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: "consumer".to_string(),
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
        peer_dependencies: Some(
            peer_names.into_iter().map(|name| (name.to_string(), "*".to_string())).collect(),
        ),
        peer_dependencies_meta: None,
    }
}

fn snapshot_with_dependency(
    alias: &str,
    dependency: pnpm_lockfile::SnapshotDepRef,
) -> pnpm_lockfile::SnapshotEntry {
    snapshot_with_dependencies([(alias, dependency)])
}

fn snapshot_with_dependencies<const DEPS: usize>(
    dependencies: [(&str, pnpm_lockfile::SnapshotDepRef); DEPS],
) -> pnpm_lockfile::SnapshotEntry {
    use pnpm_lockfile::PkgName;

    SnapshotEntry {
        dependencies: Some(
            dependencies
                .into_iter()
                .map(|(alias, dependency)| (PkgName::parse(alias).unwrap(), dependency))
                .collect(),
        ),
        ..SnapshotEntry::default()
    }
}

fn plain_dependency(ver_peer: &str) -> pnpm_lockfile::SnapshotDepRef {
    pnpm_lockfile::SnapshotDepRef::Plain(pnpm_lockfile::PkgVerPeer::from_str(ver_peer).unwrap())
}

fn alias_dependency(key: &str) -> pnpm_lockfile::SnapshotDepRef {
    pnpm_lockfile::SnapshotDepRef::Alias(pnpm_lockfile::PkgNameVerPeer::from_str(key).unwrap())
}

fn link_dependency(target: &str) -> pnpm_lockfile::SnapshotDepRef {
    pnpm_lockfile::SnapshotDepRef::Link(target.to_string())
}

/// The locked peer names a lockfile yields for an importer it does not
/// list — the union `importer_locked_peer_context` folds over every
/// snapshot.
fn locked_peer_names(wanted_lockfile: Option<&pnpm_lockfile::Lockfile>) -> HashSet<String> {
    importer_locked_peer_versions(wanted_lockfile, "missing-importer").into_keys().collect()
}

struct StubResolver {
    table: HashMap<(String, String), ResolveResult>,
    calls: Mutex<Vec<(String, String)>>,
}

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let key = (
            wanted.alias.clone().unwrap_or_default(),
            wanted.bare_specifier.clone().unwrap_or_default(),
        );
        self.calls.lock().unwrap().push(key.clone());
        let result = self.table.get(&key).cloned();
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

fn fake_result(name: &str, version: &str, manifest: serde_json::Value) -> ResolveResult {
    use pnpm_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
        latest: Some(version.to_string()),
        published_at: None,
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
fn fake_manifest(root_deps: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    fake_manifest_json(serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": root_deps,
    }))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn fake_manifest_json(json: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

fn default_opts() -> ResolveImporterOptions {
    ResolveImporterOptions {
        auto_install_peers: true,
        auto_install_peers_from_highest_match: false,
        resolve_peers_from_workspace_root: false,
        dedupe_peers: false,
        dedupe_peer_dependents: true,
        all_preferred_versions: Arc::new(PreferredVersions::new()),
        override_bare_specifier: None,
        patched_dependencies: None,
        base_opts: ResolveOptions::default(),
        pick_lowest_direct: false,
        subdep_published_by: None,
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

/// Build a [`ResolveResult`] for an `npm:`-aliased install. `local_alias`
/// is the alias the importer uses in `node_modules/` (and in
/// `parentPkgs`); `real_name`/`version` identify the resolved package.
/// Mirrors the real npm-resolver's behaviour: the result carries the
/// local alias, while `name_ver` and `id` point at the underlying
/// package.
fn aliased_fake_result(
    local_alias: &str,
    real_name: &str,
    version: &str,
    manifest: serde_json::Value,
) -> ResolveResult {
    let mut result = fake_result(real_name, version, manifest);
    result.alias = Some(local_alias.to_string());
    result
}

/// `resolutionMode` orchestration tests: assert the deps-resolver hands
/// the npm resolver the right per-depth [`ResolveOptions`]
/// (`pick_lowest_version`, `published_by`) for each mode. These cover
/// the wiring in [`TreeCtx::with_resolution_mode`] +
/// [`resolve_node`](crate::resolve_dependency_tree); the version pick
/// itself lives in the npm picker (tested there).
mod resolution_mode {
    use super::{StubResolver, default_opts, fake_manifest, fake_result};
    use crate::resolve_importer;
    use chrono::{DateTime, TimeZone, Utc};
    use pnpm_package_manifest::DependencyGroup;
    use pnpm_resolving_resolver_base::{
        ResolveFuture, ResolveOptions, ResolveResult, Resolver, WantedDependency,
    };
    use pretty_assertions::assert_eq;
    use rustc_hash::FxHashMap as HashMap;
    use std::sync::Mutex;

    /// The `(pick_lowest_version, published_by)` pair recorded per alias.
    type RecordedOpts = (bool, Option<DateTime<Utc>>);

    /// Resolver that records the [`RecordedOpts`] each `(alias, range)`
    /// query was resolved with, so a test can assert the depth-specific
    /// options the tree walker built.
    struct RecordingResolver {
        inner: StubResolver,
        seen: Mutex<HashMap<String, RecordedOpts>>,
    }

    impl RecordingResolver {
        fn new(table: HashMap<(String, String), ResolveResult>) -> Self {
            RecordingResolver {
                inner: StubResolver { table, calls: Mutex::new(Vec::new()) },
                seen: Mutex::new(HashMap::default()),
            }
        }

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
            if let Some(alias) = wanted.alias.clone() {
                self.seen
                    .lock()
                    .unwrap()
                    .insert(alias, (opts.pick_lowest_version, opts.published_by));
            }
            self.inner.resolve(wanted, opts)
        }

        fn resolve_latest<'a>(
            &'a self,
            query: &'a pnpm_resolving_resolver_base::LatestQuery,
            opts: &'a ResolveOptions,
        ) -> pnpm_resolving_resolver_base::ResolveLatestFuture<'a> {
            self.inner.resolve_latest(query, opts)
        }
    }

    fn one_dep_one_subdep_table() -> HashMap<(String, String), ResolveResult> {
        let mut table = HashMap::default();
        table.insert(
            ("direct".to_string(), "^1.0.0".to_string()),
            fake_result(
                "direct",
                "1.0.0",
                serde_json::json!({
                    "name": "direct",
                    "version": "1.0.0",
                    "dependencies": { "sub": "^2.0.0" }
                }),
            ),
        );
        table.insert(
            ("sub".to_string(), "^2.0.0".to_string()),
            fake_result("sub", "2.0.0", serde_json::json!({ "name": "sub", "version": "2.0.0" })),
        );
        table
    }

    /// `highest` (the default): both direct and transitive deps are
    /// picked highest, with the same `minimumReleaseAge` cutoff applied
    /// uniformly.
    #[tokio::test]
    async fn highest_mode_picks_highest_everywhere() {
        let resolver = RecordingResolver::new(one_dep_one_subdep_table());
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "direct": "^1.0.0" }));
        let maximum = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let mut opts = default_opts();
        opts.base_opts.published_by = Some(maximum);
        opts.pick_lowest_direct = false;
        opts.subdep_published_by = Some(maximum);

        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

        assert_eq!(resolver.opts_for("direct"), (false, Some(maximum)));
        assert_eq!(resolver.opts_for("sub"), (false, Some(maximum)));
    }

    /// `lowest-direct`: direct deps pick lowest, transitive deps pick
    /// highest, and there is no extra publish-date cutoff beyond
    /// `minimumReleaseAge` (here unset).
    #[tokio::test]
    async fn lowest_direct_mode_picks_lowest_only_for_direct_deps() {
        let resolver = RecordingResolver::new(one_dep_one_subdep_table());
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "direct": "^1.0.0" }));
        let mut opts = default_opts();
        opts.pick_lowest_direct = true;
        opts.subdep_published_by = None;

        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

        assert_eq!(resolver.opts_for("direct"), (true, None));
        assert_eq!(resolver.opts_for("sub"), (false, None));
    }

    /// `time-based`: direct deps pick lowest under the
    /// `minimumReleaseAge` cutoff; transitive deps pick highest but are
    /// constrained to the computed publish-date cutoff. The cutoff
    /// itself is computed workspace-wide in `resolve_workspace`; here we
    /// pass it in directly to assert the depth-specific threading.
    #[tokio::test]
    async fn time_based_mode_threads_cutoff_to_subdeps_only() {
        let resolver = RecordingResolver::new(one_dep_one_subdep_table());
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "direct": "^1.0.0" }));
        let maximum = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
        let mut opts = default_opts();
        opts.base_opts.published_by = Some(maximum);
        opts.pick_lowest_direct = true;
        opts.subdep_published_by = Some(cutoff);

        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

        assert_eq!(resolver.opts_for("direct"), (true, Some(maximum)));
        assert_eq!(resolver.opts_for("sub"), (false, Some(cutoff)));
    }
}
