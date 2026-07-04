use super::{PrefetchingResolver, package_id};
use crate::PrefetchContext;
use pacquet_config::Config;
use pacquet_lockfile::{LockfileResolution, TarballResolution};
use pacquet_network::ThrottledClient;
use pacquet_reporter::SilentReporter;
use pacquet_resolving_default_resolver::DefaultResolver;
use pacquet_resolving_resolver_base::{ResolveResult, WantedDependency};
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndexWriter};
use pacquet_tarball::{MemCache, SharedReportedProgressKeys};
use serde_json::json;
use std::sync::Arc;
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

fn resolver() -> PrefetchingResolver<SilentReporter> {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.cache_dir = dir.path().join("cache");
    let config = Box::leak(Box::new(config));
    let http_client = Arc::new(ThrottledClient::default());
    let mem_cache = Arc::new(MemCache::default());
    let (store_index_writer, _writer_task) = StoreIndexWriter::spawn_disabled();
    PrefetchingResolver::new(
        Box::new(DefaultResolver::new(Vec::new())),
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
    let manifest = crate::manifest_from_resolve_result(&result);

    assert!(!resolver.should_skip_prefetch(&wanted, &result));
    assert_eq!(package_id(&result, &manifest), "@pnpm.e2e/not-compatible-with-any-os@1.0.0");
}
