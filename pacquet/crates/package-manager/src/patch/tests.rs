use super::{
    PatchCandidate, PatchTarget, WritePackageForPatch, WritePackageForPatchError,
    compare_candidates, default_patch_target, executor_scripts_prepend_node_path,
    patch_candidates_from_lockfile, resolution_kind,
};
use pacquet_config::ScriptsPrependNodePath;
use pacquet_executor::ScriptsPrependNodePath as ExecScriptsPrependNodePath;
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, ComVer, GitResolution, Lockfile,
    LockfileResolution, LockfileVersion, PackageKey, PackageMetadata, RegistryResolution,
    TarballResolution, VariationsResolution,
};
use pacquet_network::{RetryOpts, ThrottledClient};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pacquet_store_dir::{StoreDir, StoreIndex, store_index_key};
use pacquet_testing_utils::registry::TestRegistry;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};

const GIT_HOSTED_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

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

fn lockfile_with_packages(keys: &[&str]) -> Lockfile {
    let packages =
        keys.iter().map(|key| (key.parse::<PackageKey>().unwrap(), registry_metadata())).collect();
    Lockfile { packages: Some(packages), ..empty_lockfile() }
}

fn registry_metadata() -> PackageMetadata {
    serde_json::from_value(json!({
        "resolution": {
            "integrity": "sha512-aGVsbG8=",
        },
    }))
    .unwrap()
}

fn lockfile_with_package_metadata(key: &str, metadata: PackageMetadata) -> Lockfile {
    Lockfile {
        packages: Some(HashMap::from([(key.parse::<PackageKey>().unwrap(), metadata)])),
        ..empty_lockfile()
    }
}

fn patch_target(raw: &str, lockfile: &Lockfile) -> PatchTarget {
    let set = patch_candidates_from_lockfile(raw, lockfile).unwrap();
    default_patch_target(&set).expect("single target")
}

fn versions(candidates: &[PatchCandidate]) -> Vec<&str> {
    candidates.iter().map(|candidate| candidate.version.as_str()).collect()
}

#[test]
fn patch_candidate_exact_version_selects_matching_installed_version() {
    let lockfile = lockfile_with_packages(&["is-positive@1.0.0"]);

    let set = patch_candidates_from_lockfile("is-positive@1.0.0", &lockfile).unwrap();
    let target = default_patch_target(&set).expect("single target");

    assert_eq!(versions(&set.versions), vec!["1.0.0"]);
    assert_eq!(versions(&set.preferred_versions), vec!["1.0.0"]);
    assert_eq!(target.alias, "is-positive");
    assert_eq!(target.version, "1.0.0");
    assert_eq!(target.bare_specifier, "1.0.0");
    assert!(!target.apply_to_all);
}

#[test]
fn patch_candidate_bare_name_single_version_applies_to_all() {
    let lockfile = lockfile_with_packages(&["is-positive@1.0.0"]);

    let set = patch_candidates_from_lockfile("is-positive", &lockfile).unwrap();
    let target = default_patch_target(&set).expect("single target");

    assert_eq!(versions(&set.preferred_versions), vec!["1.0.0"]);
    assert_eq!(target.bare_specifier, "1.0.0");
    assert!(target.apply_to_all);
}

#[test]
fn patch_candidate_range_selects_satisfying_installed_versions() {
    let lockfile = lockfile_with_packages(&["is-positive@1.0.0", "is-positive@2.0.0"]);

    let set = patch_candidates_from_lockfile("is-positive@1", &lockfile).unwrap();

    assert_eq!(versions(&set.versions), vec!["1.0.0", "2.0.0"]);
    assert_eq!(versions(&set.preferred_versions), vec!["1.0.0"]);
}

#[test]
fn patch_candidate_missing_package_reports_installed_versions_when_name_exists() {
    let lockfile = lockfile_with_packages(&["is-positive@1.0.0"]);

    let err = patch_candidates_from_lockfile("is-positive@2.0.0", &lockfile).unwrap_err();

    assert!(err.to_string().contains("1.0.0"), "error should mention installed versions: {err}");
}

#[test]
fn patch_candidate_missing_package_without_installed_versions_reports_install_hint() {
    let err = patch_candidates_from_lockfile("is-positive", &empty_lockfile()).unwrap_err();

    assert!(
        err.to_string().contains("did you forget to install is-positive?"),
        "error should mention install hint: {err}",
    );
}

#[test]
fn default_patch_target_returns_none_when_multiple_versions_match() {
    let lockfile = lockfile_with_packages(&["is-positive@1.0.0", "is-positive@2.0.0"]);
    let set = patch_candidates_from_lockfile("is-positive", &lockfile).unwrap();

    assert!(default_patch_target(&set).is_none());
}

#[test]
fn patch_candidate_reports_missing_for_non_package_specs() {
    let err = patch_candidates_from_lockfile("^1.0.0", &empty_lockfile()).unwrap_err();
    assert!(err.to_string().contains("^1.0.0"), "error should mention raw spec: {err}");

    let err = patch_candidates_from_lockfile("", &empty_lockfile()).unwrap_err();
    assert!(err.to_string().contains("install"), "error should include install hint: {err}");
}

#[test]
fn patch_candidate_detects_git_hosted_tarball_url_without_flag() {
    let tarball =
        "https://codeload.github.com/example/foo/tar.gz/0123456789abcdef0123456789abcdef01234567";
    let metadata: PackageMetadata = serde_json::from_value(json!({
        "resolution": {
            "tarball": tarball,
            "integrity": "sha512-aGVsbG8="
        },
        "version": "1.0.0",
    }))
    .unwrap();
    let lockfile = lockfile_with_package_metadata("foo@1.0.0", metadata);

    let target = patch_target("foo@1.0.0", &lockfile);

    assert_eq!(target.bare_specifier, tarball);
    assert_eq!(target.git_tarball_url.as_deref(), Some(tarball));
    assert!(!target.apply_to_all);
}

#[test]
fn patch_candidate_detects_pr_new_tarball_url_as_git_hosted() {
    let tarball = "https://pkg.pr.new/example/foo@deadbeef";
    let metadata: PackageMetadata = serde_json::from_value(json!({
        "resolution": {
            "tarball": tarball,
            "integrity": "sha512-aGVsbG8="
        },
        "version": "1.0.0",
    }))
    .unwrap();
    let lockfile = lockfile_with_package_metadata("foo@1.0.0", metadata);

    let target = patch_target("foo@1.0.0", &lockfile);

    assert_eq!(target.bare_specifier, tarball);
    assert_eq!(target.git_tarball_url.as_deref(), Some(tarball));
}

#[tokio::test]
async fn patch_extract_imports_package_files_into_empty_destination() {
    let fixture = PatchExtractFixture::new("foo", "1.0.0");
    let dest = fixture.tmp.path().join("edit");

    WritePackageForPatch {
        tarball_mem_cache: &fixture.mem_cache,
        http_client: &fixture.http_client,
        config: fixture.config,
        current_lockfile: &fixture.lockfile,
        target: &fixture.target,
        dest: &dest,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("extract package for patching");

    assert_eq!(
        std::fs::read_to_string(dest.join("package.json")).expect("package.json"),
        r#"{"name":"foo","version":"1.0.0"}"#,
    );
    assert_eq!(std::fs::read_to_string(dest.join("index.js")).expect("index.js"), "ok\n");
    assert!(!dest.join("node_modules").exists(), "edit dir should not contain wrapper deps");
}

#[cfg(unix)]
#[tokio::test]
async fn patch_extract_rejects_symlinked_destination() {
    let fixture = PatchExtractFixture::new("foo", "1.0.0");
    let outside = fixture.tmp.path().join("outside");
    let dest = fixture.tmp.path().join("edit");
    std::fs::create_dir_all(&outside).expect("create symlink target");
    std::os::unix::fs::symlink(&outside, &dest).expect("create edit symlink");

    let err = WritePackageForPatch {
        tarball_mem_cache: &fixture.mem_cache,
        http_client: &fixture.http_client,
        config: fixture.config,
        current_lockfile: &fixture.lockfile,
        target: &fixture.target,
        dest: &dest,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect_err("symlink destination must be rejected");

    assert!(err.to_string().contains("symlink"), "error should mention symlink: {err}");
    assert!(
        !outside.join("package.json").exists(),
        "patch extraction must not write through the destination symlink",
    );
}

#[tokio::test]
async fn patch_extract_records_download_in_store_index() {
    let registry = TestRegistry::start();
    let tmp = tempfile::tempdir().expect("temp dir");
    let store_dir = tmp.path().join("store");
    std::fs::create_dir_all(&store_dir).expect("create store dir");

    let mut config = pacquet_config::Config::new();
    config.registry = registry.url();
    config.store_dir = StoreDir::new(&store_dir);
    let config: &'static pacquet_config::Config = Box::leak(Box::new(config));

    let http_client = Arc::new(ThrottledClient::new_for_installs());
    let resolved = resolve_registry_fixture(
        &config.registry,
        tmp.path(),
        Arc::clone(&http_client),
        "@pnpm.e2e/hello-world-js-bin",
        "1.0.0",
    )
    .await;
    let name_ver = resolved.name_ver.as_ref().expect("npm resolver fills name/version");
    let package_id = name_ver.to_string();
    let integrity = resolved.resolution.integrity().expect("registry fixture has integrity");
    let store_index_key = store_index_key(&integrity.to_string(), &package_id);
    let lockfile = lockfile_with_package_metadata(
        &package_id,
        PackageMetadata {
            resolution: resolved.resolution,
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
        },
    );
    let target = patch_target(&package_id, &lockfile);
    let dest = tmp.path().join("edit");

    WritePackageForPatch {
        tarball_mem_cache: &pacquet_tarball::MemCache::default(),
        http_client: http_client.as_ref(),
        config,
        current_lockfile: &lockfile,
        target: &target,
        dest: &dest,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("extract registry package");

    let store_index = StoreIndex::shared_readonly_in(&config.store_dir)
        .expect("patch extraction should create a store index");
    let indexed_package =
        store_index.lock().expect("store index lock").get(&store_index_key).expect("read row");
    assert!(indexed_package.is_some(), "store index row should exist for {store_index_key}");
}

#[tokio::test]
async fn patch_extract_replaces_existing_empty_destination() {
    let fixture = PatchExtractFixture::new("foo", "1.0.0");
    let dest = fixture.tmp.path().join("edit");
    std::fs::create_dir_all(&dest).expect("create empty edit dir");

    WritePackageForPatch {
        tarball_mem_cache: &fixture.mem_cache,
        http_client: &fixture.http_client,
        config: fixture.config,
        current_lockfile: &fixture.lockfile,
        target: &fixture.target,
        dest: &dest,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("extract into empty dir");

    assert!(dest.join("package.json").is_file());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn patch_extract_git_hosted_tarball_runs_packlist() {
    let fixture = PatchExtractFixture::new_git_hosted("foo", "1.0.0");
    let dest = fixture.tmp.path().join("edit");

    WritePackageForPatch {
        tarball_mem_cache: &fixture.mem_cache,
        http_client: &fixture.http_client,
        config: fixture.config,
        current_lockfile: &fixture.lockfile,
        target: &fixture.target,
        dest: &dest,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("extract git-hosted package for patching");

    assert_eq!(
        std::fs::read_to_string(dest.join("package.json")).expect("package.json"),
        r#"{"name":"foo","version":"1.0.0","files":["index.js"]}"#,
    );
    assert_eq!(std::fs::read_to_string(dest.join("index.js")).expect("index.js"), "ok\n");
    assert!(!dest.join("ignore.txt").exists(), "packlist should filter ignored files");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn patch_extract_url_inferred_git_hosted_tarball_runs_packlist() {
    let fixture = PatchExtractFixture::new_git_hosted_without_flag("foo", "1.0.0");
    let dest = fixture.tmp.path().join("edit");

    WritePackageForPatch {
        tarball_mem_cache: &fixture.mem_cache,
        http_client: &fixture.http_client,
        config: fixture.config,
        current_lockfile: &fixture.lockfile,
        target: &fixture.target,
        dest: &dest,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .expect("extract URL-inferred git-hosted package for patching");

    assert_eq!(
        std::fs::read_to_string(dest.join("package.json")).expect("package.json"),
        r#"{"name":"foo","version":"1.0.0","files":["index.js"]}"#,
    );
    assert_eq!(std::fs::read_to_string(dest.join("index.js")).expect("index.js"), "ok\n");
    assert!(!dest.join("ignore.txt").exists(), "packlist should filter ignored files");
}

#[tokio::test]
async fn patch_extract_rejects_unsupported_resolution_shape() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = pacquet_config::Config::new();
    config.store_dir = tmp.path().join("store").into();
    let config: &'static pacquet_config::Config = Box::leak(Box::new(config));
    let metadata: PackageMetadata = serde_json::from_value(json!({
        "resolution": {
            "type": "directory",
            "directory": "../foo",
        },
        "version": "1.0.0",
    }))
    .unwrap();
    let lockfile = lockfile_with_package_metadata("foo@1.0.0", metadata);
    let target = patch_target("foo@1.0.0", &lockfile);
    let mem_cache = pacquet_tarball::MemCache::default();
    let http_client = pacquet_network::ThrottledClient::default();
    let dest = tmp.path().join("edit");

    let err = WritePackageForPatch {
        tarball_mem_cache: &mem_cache,
        http_client: &http_client,
        config,
        current_lockfile: &lockfile,
        target: &target,
        dest: &dest,
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .unwrap_err();

    assert!(err.to_string().contains("directory"), "error should name unsupported shape: {err}");
}

#[tokio::test]
async fn patch_extract_rejects_missing_package_metadata() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = pacquet_config::Config::new();
    config.store_dir = tmp.path().join("store").into();
    let config: &'static pacquet_config::Config = Box::leak(Box::new(config));
    let mem_cache = pacquet_tarball::MemCache::default();
    let http_client = pacquet_network::ThrottledClient::default();
    let target = PatchTarget {
        alias: "missing".to_string(),
        version: "1.0.0".to_string(),
        bare_specifier: "1.0.0".to_string(),
        apply_to_all: false,
        git_tarball_url: None,
        package_key: "missing@1.0.0".parse().expect("package key"),
    };

    let err = WritePackageForPatch {
        tarball_mem_cache: &mem_cache,
        http_client: &http_client,
        config,
        current_lockfile: &empty_lockfile(),
        target: &target,
        dest: &tmp.path().join("edit"),
    }
    .run::<pacquet_reporter::SilentReporter>()
    .await
    .unwrap_err();

    assert!(
        matches!(err, WritePackageForPatchError::MissingPackageMetadata { .. }),
        "missing metadata should be reported, got {err:?}",
    );
}

#[test]
fn executor_scripts_prepend_node_path_maps_all_variants() {
    assert_eq!(
        executor_scripts_prepend_node_path(ScriptsPrependNodePath::Always),
        ExecScriptsPrependNodePath::Always,
    );
    assert_eq!(
        executor_scripts_prepend_node_path(ScriptsPrependNodePath::Never),
        ExecScriptsPrependNodePath::Never,
    );
    assert_eq!(
        executor_scripts_prepend_node_path(ScriptsPrependNodePath::WarnOnly),
        ExecScriptsPrependNodePath::WarnOnly,
    );
}

#[test]
fn compare_candidates_orders_semver_before_non_semver() {
    let candidate = |version: &str| PatchCandidate {
        name: "foo".to_string(),
        version: version.to_string(),
        git_tarball_url: None,
        package_key: "foo@1.0.0".parse().expect("package key"),
    };

    assert_eq!(
        compare_candidates(&candidate("1.0.0"), &candidate("workspace:*")),
        std::cmp::Ordering::Less,
    );
    assert_eq!(
        compare_candidates(&candidate("workspace:*"), &candidate("1.0.0")),
        std::cmp::Ordering::Greater,
    );
    assert_eq!(
        compare_candidates(&candidate("workspace:*"), &candidate("npm:foo@1.0.0")),
        std::cmp::Ordering::Equal,
    );
}

#[test]
fn resolution_kind_names_non_patchable_resolution_shapes() {
    let tarball = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        integrity: Some("sha512-aGVsbG8=".parse().expect("integrity")),
        git_hosted: None,
        path: None,
    });
    assert_eq!(resolution_kind(&tarball), "tarball");

    let registry = LockfileResolution::Registry(RegistryResolution {
        integrity: "sha512-aGVsbG8=".parse().expect("integrity"),
    });
    assert_eq!(resolution_kind(&registry), "registry");

    let git = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/example/foo.git".to_string(),
        commit: "deadbeef".to_string(),
        path: None,
    });
    assert_eq!(resolution_kind(&git), "git");

    let binary = LockfileResolution::Binary(BinaryResolution {
        url: "https://nodejs.org/dist/v1.0.0/node.tar.gz".to_string(),
        integrity: "sha512-aGVsbG8=".parse().expect("integrity"),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    });
    assert_eq!(resolution_kind(&binary), "binary");

    let variations = LockfileResolution::Variations(VariationsResolution { variants: Vec::new() });
    assert_eq!(resolution_kind(&variations), "variations");
}

struct PatchExtractFixture {
    tmp: tempfile::TempDir,
    mem_cache: pacquet_tarball::MemCache,
    http_client: pacquet_network::ThrottledClient,
    config: &'static pacquet_config::Config,
    lockfile: Lockfile,
    target: PatchTarget,
}

impl PatchExtractFixture {
    fn new(name: &str, version: &str) -> Self {
        Self::new_with_options(name, version, false, false)
    }

    fn new_git_hosted(name: &str, version: &str) -> Self {
        Self::new_with_options(name, version, true, true)
    }

    fn new_git_hosted_without_flag(name: &str, version: &str) -> Self {
        Self::new_with_options(name, version, true, false)
    }

    fn new_with_options(
        name: &str,
        version: &str,
        use_git_hosted_url: bool,
        git_hosted_flag: bool,
    ) -> Self {
        use pacquet_tarball::CacheValue;
        use std::{collections::HashMap, path::PathBuf, sync::Arc};

        let tmp = tempfile::tempdir().expect("temp dir");
        let store_dir = tmp.path().join("store");
        std::fs::create_dir_all(&store_dir).expect("create store dir");
        let pkg_json = store_dir.join("pkg-json");
        let index = store_dir.join("index");
        let manifest = if use_git_hosted_url {
            format!(r#"{{"name":"{name}","version":"{version}","files":["index.js"]}}"#)
        } else {
            format!(r#"{{"name":"{name}","version":"{version}"}}"#)
        };
        std::fs::write(&pkg_json, manifest).expect("write package json");
        std::fs::write(&index, "ok\n").expect("write index");
        let ignored = store_dir.join("ignore");
        std::fs::write(&ignored, "do not publish\n").expect("write ignored file");

        let mut config = pacquet_config::Config::new();
        config.registry = "https://registry.test/".to_string();
        config.store_dir = store_dir.into();
        config.offline = true;
        let config: &'static pacquet_config::Config = Box::leak(Box::new(config));

        let key = format!("{name}@{version}");
        let lockfile = lockfile_with_package_metadata(
            &key,
            if use_git_hosted_url {
                git_hosted_metadata(git_hosted_flag)
            } else {
                registry_metadata()
            },
        );
        let target = patch_target(&key, &lockfile);
        let tarball_url = if use_git_hosted_url {
            git_hosted_tarball(name)
        } else {
            format!("https://registry.test/{name}/-/{name}-{version}.tgz")
        };
        let seeded: HashMap<String, PathBuf> = HashMap::from([
            ("package.json".to_string(), pkg_json),
            ("index.js".to_string(), index),
            ("ignore.txt".to_string(), ignored),
        ]);
        let mem_cache = pacquet_tarball::MemCache::default();
        mem_cache.insert(
            tarball_url,
            Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded)))),
        );

        Self {
            tmp,
            mem_cache,
            http_client: pacquet_network::ThrottledClient::default(),
            config,
            lockfile,
            target,
        }
    }
}

async fn resolve_registry_fixture(
    registry: &str,
    cache_dir: &std::path::Path,
    http_client: Arc<ThrottledClient>,
    alias: &str,
    range: &str,
) -> pacquet_resolving_resolver_base::ResolveResult {
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), registry.to_string());
    let resolver = NpmResolver {
        registries,
        named_registries: HashMap::new(),
        http_client,
        auth_headers: Arc::default(),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(cache_dir.to_path_buf()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: true,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let wanted = WantedDependency {
        alias: Some(alias.to_string()),
        bare_specifier: Some(range.to_string()),
        ..WantedDependency::default()
    };
    resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect("resolve succeeds against fixture registry")
        .expect("npm resolver claims fixture")
}

fn git_hosted_metadata(git_hosted_flag: bool) -> PackageMetadata {
    let tarball = git_hosted_tarball("foo");
    let resolution = if git_hosted_flag {
        json!({
            "tarball": tarball,
            "integrity": "sha512-aGVsbG8=",
            "gitHosted": true,
        })
    } else {
        json!({
            "tarball": tarball,
            "integrity": "sha512-aGVsbG8=",
        })
    };
    serde_json::from_value(json!({
        "resolution": resolution,
        "version": "1.0.0",
    }))
    .unwrap()
}

fn git_hosted_tarball(name: &str) -> String {
    format!("https://codeload.github.com/example/{name}/tar.gz/{GIT_HOSTED_COMMIT}")
}
