use super::PrefetchingResolver;
use crate::PrefetchContext;
use pnpm_config::Config;
use pnpm_lockfile::{DirectoryResolution, LockfileResolution, TarballResolution};
use pnpm_network::ThrottledClient;
use pnpm_reporter::SilentReporter;
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver,
    WantedDependency,
};
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreIndexWriter};
use pnpm_tarball::{MemCache, SharedReportedProgressKeys, package_mem_cache_key};
use serde_json::json;
use std::{io::Write, path::Path, sync::Arc};
use tempfile::tempdir;

fn result_with_manifest(name: &str, manifest: serde_json::Value) -> ResolveResult {
    let id = format!("{name}@1.0.0");
    ResolveResult {
        id: id.clone().into(),
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://registry.example/not-compatible.tgz".to_string(),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: None,
        policy_violation: None,
        package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
            name_ver: Some(id.parse().unwrap()),
            latest: None,
            published_at: None,
            manifest: Some(Arc::new(manifest)),
        },
    }
}

fn result_without_manifest(name: &str) -> ResolveResult {
    let mut result = result_with_manifest(name, json!({}));
    result.package.manifest = None;
    result
}

fn alias_tarball_result(alias: &str, manifest: serde_json::Value) -> ResolveResult {
    ResolveResult {
        id: "https://registry.example/not-compatible.tgz".into(),
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://registry.example/not-compatible.tgz".to_string(),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "tarball".to_string(),
        normalized_bare_specifier: None,
        alias: Some(alias.to_string()),
        policy_violation: None,
        package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(Arc::new(manifest)),
        },
    }
}

fn anonymous_tarball_result(manifest: serde_json::Value) -> ResolveResult {
    ResolveResult {
        id: "https://registry.example/not-compatible.tgz".into(),
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://registry.example/not-compatible.tgz".to_string(),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "tarball".to_string(),
        normalized_bare_specifier: None,
        alias: None,
        policy_violation: None,
        package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(Arc::new(manifest)),
        },
    }
}

#[derive(Clone)]
struct FixedResolver {
    result: ResolveResult,
}

impl Resolver for FixedResolver {
    fn resolve<'a>(
        &'a self,
        _wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let result = self.result.clone();
        Box::pin(async move { Ok(Some(result)) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

fn resolver_with_inner(
    dir: &Path,
    inner: Box<dyn Resolver>,
) -> PrefetchingResolver<SilentReporter> {
    resolver_with_prefetch(dir, inner, true)
}

fn resolver_with_prefetch(
    dir: &Path,
    inner: Box<dyn Resolver>,
    prefetch_downloads: bool,
) -> PrefetchingResolver<SilentReporter> {
    let mut config = Config::new();
    config.store_dir = dir.join("store").into();
    config.cache_dir = dir.join("cache");
    // A test that drives the fetch to a failure should report it, not
    // spend a minute backing off from a mock server.
    config.fetch_retries = 0;
    let config = Box::leak(Box::new(config));
    let http_client = Arc::new(ThrottledClient::default());
    let mem_cache = Arc::new(MemCache::default());
    let (store_index_writer, _writer_task) = StoreIndexWriter::spawn_disabled();
    PrefetchingResolver::new(
        inner,
        PrefetchContext {
            http_client: &http_client,
            mem_cache: &mem_cache,
            config,
            requester: "/project",
            supported_architectures: None,
            progress_reported: &SharedReportedProgressKeys::default(),
            store: crate::PrefetchStoreRefs {
                index: None,
                index_writer: Some(&store_index_writer),
                verified_files_cache: &SharedVerifiedFilesCache::default(),
            },
            policy: crate::PrefetchPolicy { downloads: prefetch_downloads, custom_session: None },
        },
    )
}

fn resolver() -> PrefetchingResolver<SilentReporter> {
    let dir = tempdir().unwrap();
    resolver_with_inner(dir.path(), Box::new(DefaultResolver::new(Vec::new())))
}

fn minimal_tarball(name: &str, version: &str) -> Vec<u8> {
    let manifest = serde_json::json!({ "name": name, "version": version }).to_string();
    let manifest = manifest.as_bytes();

    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("package/package.json").expect("set tar entry path");
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, manifest).expect("append package.json to tar");
    let tar_bytes = builder.into_inner().expect("finish tar");

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).expect("gzip tar");
    encoder.finish().expect("finish gzip")
}

#[tokio::test]
async fn skips_prefetch_for_unsupported_optional_manifest() {
    let resolver = resolver();
    let wanted = WantedDependency { optional: Some(true), ..WantedDependency::default() };
    let result = result_with_manifest(
        "@pnpm.e2e/not-compatible-with-any-os",
        json!({
            "name": "@pnpm.e2e/not-compatible-with-any-os",
            "version": "1.0.0",
            "os": ["this-os-does-not-exist"]
        }),
    );

    assert!(resolver.should_skip_prefetch(&wanted, &result));
}

#[tokio::test]
async fn skips_prefetch_for_platform_inferred_from_optional_name() {
    let resolver = resolver();
    let wanted = WantedDependency { optional: Some(true), ..WantedDependency::default() };
    let result = result_with_manifest(
        "@esbuild/openharmony-arm64",
        json!({
            "name": "@esbuild/openharmony-arm64",
            "version": "1.0.0"
        }),
    );

    assert!(resolver.should_skip_prefetch(&wanted, &result));
}

#[tokio::test]
async fn skips_prefetch_for_manifestless_platform_inferred_name() {
    let resolver = resolver();
    let wanted = WantedDependency { optional: Some(true), ..WantedDependency::default() };
    let result = result_without_manifest("@esbuild/openharmony-arm64");

    assert!(resolver.should_skip_prefetch(&wanted, &result));
}

#[tokio::test]
async fn skips_prefetch_using_alias_when_manifest_name_missing() {
    let resolver = resolver();
    let wanted = WantedDependency {
        alias: Some("@esbuild/openharmony-arm64".to_string()),
        optional: Some(true),
        ..WantedDependency::default()
    };
    let result = alias_tarball_result("@esbuild/openharmony-arm64", json!({ "version": "1.0.0" }));

    assert!(resolver.should_skip_prefetch(&wanted, &result));
}

#[tokio::test]
async fn skips_prefetch_for_anonymous_manifest_with_explicit_platform_constraint() {
    let resolver = resolver();
    let wanted = WantedDependency { optional: Some(true), ..WantedDependency::default() };
    let result = anonymous_tarball_result(json!({
        "version": "1.0.0",
        "os": ["this-os-does-not-exist"]
    }));

    assert!(resolver.should_skip_prefetch(&wanted, &result));
}

#[tokio::test]
async fn resolve_populates_integrity_before_skipping_optional_prefetch() {
    let dir = tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let tarball_path = "/not-compatible-1.0.0.tgz";
    let tarball_url = format!("{}{tarball_path}", server.url());
    let get_mock = server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(minimal_tarball("not-compatible", "1.0.0"))
        .expect(1)
        .create_async()
        .await;
    let mut result = result_with_manifest(
        "not-compatible",
        json!({
            "name": "not-compatible",
            "version": "1.0.0",
            "os": ["this-os-does-not-exist"]
        }),
    );
    result.resolution = LockfileResolution::Tarball(TarballResolution {
        integrity: None,
        tarball: tarball_url,
        revision: None,
        git_hosted: None,
        path: None,
    });
    let resolver = resolver_with_inner(dir.path(), Box::new(FixedResolver { result }));
    let wanted = WantedDependency { optional: Some(true), ..WantedDependency::default() };

    let resolved = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect("resolve succeeds")
        .expect("resolver returns a result");

    let LockfileResolution::Tarball(tarball) = resolved.resolution else {
        panic!("expected tarball resolution");
    };
    assert!(tarball.integrity.is_some(), "unsupported optional tarball still needs integrity");
    get_mock.assert_async().await;
}

#[tokio::test]
async fn keeps_prefetch_check_off_non_tarball_resolutions() {
    let resolver = resolver();
    let wanted = WantedDependency { optional: Some(true), ..WantedDependency::default() };
    let mut result = result_with_manifest(
        "@pnpm.e2e/not-compatible-with-any-os",
        json!({
            "name": "@pnpm.e2e/not-compatible-with-any-os",
            "version": "1.0.0",
            "os": ["this-os-does-not-exist"]
        }),
    );
    result.resolution = LockfileResolution::Directory(DirectoryResolution {
        directory: "../not-compatible".to_string(),
    });

    assert!(!resolver.should_skip_prefetch(&wanted, &result));
}

#[tokio::test]
async fn keeps_prefetch_for_required_manifest() {
    let resolver = resolver();
    let wanted = WantedDependency { optional: Some(false), ..WantedDependency::default() };
    let result = result_with_manifest(
        "@pnpm.e2e/not-compatible-with-any-os",
        json!({
            "name": "@pnpm.e2e/not-compatible-with-any-os",
            "version": "1.0.0",
            "os": ["this-os-does-not-exist"]
        }),
    );

    assert!(!resolver.should_skip_prefetch(&wanted, &result));
}

const PINNED_INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn integrity_pinned_result(tarball_url: &str) -> ResolveResult {
    let mut result =
        result_with_manifest("pinned", json!({ "name": "pinned", "version": "1.0.0" }));
    result.resolution = LockfileResolution::Tarball(TarballResolution {
        integrity: Some(PINNED_INTEGRITY.parse().unwrap()),
        tarball: tarball_url.to_string(),
        revision: None,
        git_hosted: None,
        path: None,
    });
    result
}

/// <https://github.com/pnpm/pnpm/issues/13547>
#[tokio::test]
async fn populates_integrity_with_prefetching_off() {
    let dir = tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let tarball_path = "/unpinned-1.0.0.tgz";
    let get_mock = server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(minimal_tarball("unpinned", "1.0.0"))
        .expect(1)
        .create_async()
        .await;
    let mut result = result_with_manifest("unpinned", json!({}));
    result.resolution = LockfileResolution::Tarball(TarballResolution {
        integrity: None,
        tarball: format!("{}{tarball_path}", server.url()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let resolver = resolver_with_prefetch(dir.path(), Box::new(FixedResolver { result }), false);

    let resolved = resolver
        .resolve(&WantedDependency::default(), &ResolveOptions::default())
        .await
        .expect("resolve succeeds")
        .expect("resolver returns a result");

    let LockfileResolution::Tarball(tarball) = resolved.resolution else {
        panic!("expected tarball resolution");
    };
    assert!(tarball.integrity.is_some(), "an unpinned tarball still needs its integrity");
    get_mock.assert_async().await;
}

#[tokio::test]
async fn skips_the_background_download_with_prefetching_off() {
    let dir = tempdir().unwrap();
    let tarball_url = "https://registry.example/pinned-1.0.0.tgz";
    let resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: integrity_pinned_result(tarball_url) }),
        false,
    );

    resolver
        .resolve(&WantedDependency::default(), &ResolveOptions::default())
        .await
        .expect("resolve succeeds")
        .expect("resolver returns a result");

    assert!(resolver.spawned_downloads.is_empty(), "no download may be claimed");
}

#[tokio::test]
async fn claims_the_background_download_with_prefetching_on() {
    let dir = tempdir().unwrap();
    let tarball_url = "https://registry.example/pinned-1.0.0.tgz";
    let resolver = resolver_with_inner(
        dir.path(),
        Box::new(FixedResolver { result: integrity_pinned_result(tarball_url) }),
    );

    resolver
        .resolve(&WantedDependency::default(), &ResolveOptions::default())
        .await
        .expect("resolve succeeds")
        .expect("resolver returns a result");

    assert!(
        resolver.spawned_downloads.contains(&package_mem_cache_key(
            tarball_url,
            Some(&PINNED_INTEGRITY.parse().expect("parse integrity")),
            false,
        )),
        "the download must be claimed",
    );
}

fn tarball_with_a_dependency(name: &str) -> Vec<u8> {
    let manifest = serde_json::json!({
        "name": name,
        "version": "1.0.0",
        "dependencies": { "ms": "2.1.2" },
    })
    .to_string();
    let manifest = manifest.as_bytes();

    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("package/package.json").expect("set tar entry path");
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, manifest).expect("append package.json to tar");
    let tar_bytes = builder.into_inner().expect("finish tar");

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).expect("gzip tar");
    encoder.finish().expect("finish gzip")
}

fn manifestless_tarball_result(tarball_url: &str, integrity: &str) -> ResolveResult {
    manifestless_tarball_result_with_revision(tarball_url, integrity, None)
}

fn manifestless_tarball_result_with_revision(
    tarball_url: &str,
    integrity: &str,
    revision: Option<u64>,
) -> ResolveResult {
    let mut result = result_without_manifest("pinned");
    result.resolution = LockfileResolution::Tarball(TarballResolution {
        integrity: Some(integrity.parse().expect("parse integrity")),
        tarball: tarball_url.to_string(),
        revision: revision.map(|revision| revision.try_into().expect("build revision")),
        git_hosted: None,
        path: None,
    });
    result
}

/// A revision's protocol allows exactly one GET, so the read that
/// recovers the manifest has to be that GET and has to publish where the
/// install pass looks. <https://github.com/pnpm/pnpm/issues/15021>
#[tokio::test]
async fn reads_a_revision_addressed_tarball_under_its_own_network_policy() {
    let dir = tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let tarball_path = "/revision-1.0.0.tgz";
    let body = tarball_with_a_dependency("revision");
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&body)
        .result()
        .to_string();
    let tarball_url = format!("{}{tarball_path}", server.url());
    let get_mock = server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let result = manifestless_tarball_result_with_revision(&tarball_url, &integrity, Some(1));
    let resolver = resolver_with_inner(dir.path(), Box::new(FixedResolver { result }));

    let resolved = resolver
        .resolve(&WantedDependency::default(), &ResolveOptions::default())
        .await
        .expect("resolve succeeds")
        .expect("resolver returns a result");

    let manifest = resolved.package.manifest.expect("the bundled manifest fills the gap");
    assert_eq!(dbg!(&manifest)["dependencies"]["ms"], json!("2.1.2"));
    get_mock.assert_async().await;
    let cache_key =
        package_mem_cache_key(&tarball_url, Some(&integrity.parse().expect("integrity")), true);
    assert!(
        resolver.ctx.mem_cache.contains_key(&cache_key),
        "the read publishes under the revision-addressed identity",
    );
    assert!(
        resolver.spawned_downloads.contains(&cache_key),
        "and claims it, so the prefetch spends no second GET",
    );
}

/// <https://github.com/pnpm/pnpm/issues/15000>
#[tokio::test]
async fn reads_the_manifest_of_a_pinned_tarball_the_resolver_left_without_one() {
    let dir = tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let tarball_path = "/pinned-1.0.0.tgz";
    let body = tarball_with_a_dependency("pinned");
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&body)
        .result()
        .to_string();
    let get_mock = server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let result =
        manifestless_tarball_result(&format!("{}{tarball_path}", server.url()), &integrity);
    let resolver = resolver_with_prefetch(dir.path(), Box::new(FixedResolver { result }), false);

    let resolved = resolver
        .resolve(&WantedDependency::default(), &ResolveOptions::default())
        .await
        .expect("resolve succeeds")
        .expect("resolver returns a result");

    let manifest = resolved.package.manifest.expect("the bundled manifest fills the gap");
    assert_eq!(dbg!(&manifest)["dependencies"]["ms"], json!("2.1.2"));
    get_mock.assert_async().await;
    // One download, not two: the read publishes its extraction under the hash
    // the resolution records, and claims that identity so the prefetch path
    // does not fetch the same archive again.
    // <https://github.com/pnpm/pnpm/issues/15021>
    let cache_key = package_mem_cache_key(
        &format!("{}{tarball_path}", server.url()),
        Some(&integrity.parse().expect("parse integrity")),
        false,
    );
    assert!(
        resolver.ctx.mem_cache.contains_key(&cache_key),
        "the read publishes its extraction under the archive's own hash",
    );
    assert!(resolver.spawned_downloads.contains(&cache_key), "and claims the download");
}

#[tokio::test]
async fn refuses_a_manifest_read_from_a_tarball_that_fails_its_integrity() {
    let dir = tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let tarball_path = "/tampered-1.0.0.tgz";
    let get_mock = server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(tarball_with_a_dependency("tampered"))
        .create_async()
        .await;
    let result = manifestless_tarball_result(
        &format!("{}{tarball_path}", server.url()),
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
    );
    let resolver = resolver_with_prefetch(dir.path(), Box::new(FixedResolver { result }), false);

    let error = resolver
        .resolve(&WantedDependency::default(), &ResolveOptions::default())
        .await
        .expect_err("a tarball that fails its pinned hash must not supply a manifest");

    assert!(dbg!(error.to_string()).contains("Integrity check failed"), "got: {error}");
    get_mock.assert_async().await;
}

/// A tarball URL may itself end in `:sha512-…`, so concatenating a URL and
/// an integrity is not enough to tell a pinned read from an unpinned one.
#[tokio::test]
async fn a_url_spelling_another_url_and_its_integrity_gets_its_own_cache_cell() {
    let dir = tempdir().unwrap();
    let integrity = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    let pinned = manifestless_tarball_result("https://registry.example/x.tgz", integrity);
    let mut unpinned = result_without_manifest("collider");
    unpinned.resolution = LockfileResolution::Tarball(TarballResolution {
        integrity: None,
        tarball: format!("https://registry.example/x.tgz:{integrity}"),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let resolver =
        resolver_with_prefetch(dir.path(), Box::new(DefaultResolver::new(Vec::new())), false);

    let key = |result: &ResolveResult| {
        let LockfileResolution::Tarball(tarball) = &result.resolution else {
            panic!("expected tarball resolution");
        };
        resolver.tarball_metadata_cache_key(result, tarball, "collider@1.0.0").expect("build key")
    };

    assert_ne!(dbg!(key(&pinned)), dbg!(key(&unpinned)));
}
