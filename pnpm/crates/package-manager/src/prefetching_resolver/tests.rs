use super::PrefetchingResolver;
use crate::PrefetchContext;
use pacquet_config::Config;
use pacquet_lockfile::{DirectoryResolution, LockfileResolution, TarballResolution};
use pacquet_network::ThrottledClient;
use pacquet_reporter::SilentReporter;
use pacquet_resolving_default_resolver::DefaultResolver;
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver,
    WantedDependency,
};
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndexWriter};
use pacquet_tarball::{MemCache, SharedReportedProgressKeys};
use serde_json::json;
use std::{io::Write, path::Path, sync::Arc};
use tempfile::tempdir;

fn result_with_manifest(name: &str, manifest: serde_json::Value) -> ResolveResult {
    let id = format!("{name}@1.0.0");
    ResolveResult {
        id: id.clone().into(),
        name_ver: Some(id.parse().unwrap()),
        latest: None,
        published_at: None,
        manifest: Some(Arc::new(manifest)),
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://registry.example/not-compatible.tgz".to_string(),
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: None,
        policy_violation: None,
    }
}

fn result_without_manifest(name: &str) -> ResolveResult {
    let mut result = result_with_manifest(name, json!({}));
    result.manifest = None;
    result
}

fn alias_tarball_result(alias: &str, manifest: serde_json::Value) -> ResolveResult {
    ResolveResult {
        id: "https://registry.example/not-compatible.tgz".into(),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(Arc::new(manifest)),
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://registry.example/not-compatible.tgz".to_string(),
            git_hosted: None,
            path: None,
        }),
        resolved_via: "tarball".to_string(),
        normalized_bare_specifier: None,
        alias: Some(alias.to_string()),
        policy_violation: None,
    }
}

fn anonymous_tarball_result(manifest: serde_json::Value) -> ResolveResult {
    ResolveResult {
        id: "https://registry.example/not-compatible.tgz".into(),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(Arc::new(manifest)),
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://registry.example/not-compatible.tgz".to_string(),
            git_hosted: None,
            path: None,
        }),
        resolved_via: "tarball".to_string(),
        normalized_bare_specifier: None,
        alias: None,
        policy_violation: None,
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
    let mut config = Config::new();
    config.store_dir = dir.join("store").into();
    config.cache_dir = dir.join("cache");
    let config = Box::leak(Box::new(config));
    let http_client = Arc::new(ThrottledClient::default());
    let mem_cache = Arc::new(MemCache::default());
    let (store_index_writer, _writer_task) = StoreIndexWriter::spawn_disabled();
    PrefetchingResolver::new(
        inner,
        PrefetchContext {
            http_client: &http_client,
            mem_cache: &mem_cache,
            store_index: None,
            store_index_writer: Some(&store_index_writer),
            verified_files_cache: &SharedVerifiedFilesCache::default(),
            config,
            requester: "/project",
            supported_architectures: None,
            progress_reported: &SharedReportedProgressKeys::default(),
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
