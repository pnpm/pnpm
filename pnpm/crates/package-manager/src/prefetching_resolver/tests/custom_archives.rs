use super::{
    FixedResolver, manifestless_tarball_result, resolver, resolver_with_prefetch,
    result_without_manifest, tarball_with_a_dependency,
};
use pnpm_lockfile::{LockfileResolution, TarballResolution};
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pnpm_tarball::package_mem_cache_key;
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn reads_a_relative_local_tarball_without_a_manifest() {
    let dir = tempdir().unwrap();
    let body = tarball_with_a_dependency("pinned");
    let integrity = ssri::Integrity::from(&body).to_string();
    std::fs::write(dir.path().join("package.tgz"), body).unwrap();
    let url = format!("file:.{}package.tgz", std::path::MAIN_SEPARATOR);
    let result = manifestless_tarball_result(&url, &integrity);
    let resolver = resolver_with_prefetch(dir.path(), Box::new(FixedResolver { result }), true);
    let mut opts = ResolveOptions::default();
    opts.project.lockfile_dir = dir.path().to_owned();
    let resolved = resolver
        .resolve(&WantedDependency::default(), &opts)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.package.manifest.unwrap()["dependencies"]["ms"], json!("2.1.2"));
    let LockfileResolution::Tarball(tarball) = resolved.resolution else { panic!("tarball") };
    assert_eq!(tarball.tarball, url);
    assert_eq!(resolver.spawned_downloads.len(), 1);
    assert_eq!(tarball.integrity.unwrap().to_string(), integrity);
}

#[tokio::test]
async fn git_archive_manifests_are_cached_by_subdirectory() {
    let dir = tempdir().unwrap();
    let url =
        "https://codeload.github.com/example/repo/tar.gz/0123456789abcdef0123456789abcdef01234567";
    let integrity = ssri::Integrity::from(b"archive").to_string();
    let resolver = resolver();
    let mut files = std::collections::HashMap::new();
    for name in ["first", "second"] {
        let path = dir.path().join(format!("{name}.json"));
        std::fs::write(&path, json!({"name": name, "dependencies": {name: "1.0.0"}}).to_string())
            .unwrap();
        files.insert(format!("packages/{name}/package.json"), path);
    }
    resolver.ctx.mem_cache.insert(
        package_mem_cache_key(url, Some(&integrity.parse().unwrap()), false),
        Arc::new(tokio::sync::RwLock::new(pnpm_tarball::CacheValue::Available(Arc::new(
            pnpm_tarball::CachedTarball {
                files: Arc::new(files),
                manifest: Some(json!({"name": "root"})),
            },
        )))),
    );
    for name in ["first", "second"] {
        let mut result = manifestless_tarball_result(url, &integrity);
        let LockfileResolution::Tarball(tarball) = &mut result.resolution else {
            panic!("tarball")
        };
        tarball.path = Some(format!("/packages/{name}"));
        resolver.populate_missing_tarball_metadata(&mut result, dir.path()).await.unwrap();
        assert_eq!(result.package.manifest.unwrap()["name"], json!(name));
        assert_eq!(
            result.resolution
                .integrity()
                .unwrap()
                .to_string(),
            integrity,
        );
    }
}

struct LocalArchiveFetcher {
    path: Option<&'static str>,
}

#[async_trait::async_trait]
impl pnpm_hooks::CustomFetcher for LocalArchiveFetcher {
    async fn can_fetch(
        &self,
        _id: &str,
        _resolution: serde_json::Value,
    ) -> Result<bool, pnpm_hooks::HookError> {
        Ok(true)
    }
    async fn fetch(
        &self,
        _id: &str,
        _resolution: serde_json::Value,
        _opts: serde_json::Value,
    ) -> Result<serde_json::Value, pnpm_hooks::HookError> {
        Ok(
            json!({"delegate": {"tarball": "file:./repo.tgz", "path": self.path, "gitHosted": self.path.is_some()}}),
        )
    }
}

/// Stands in for a `canFetch` that caches a machine-local lookup on the
/// resolution it is handed, which the JS adapter carries forward to `fetch`.
struct ScratchWritingFetcher;

/// The same, for a fetcher that ends up declining the package.
struct DecliningScratchWritingFetcher;

fn with_scratch_field(mut resolution: serde_json::Value) -> serde_json::Value {
    if let Some(object) = resolution.as_object_mut() {
        object.insert("_localCache".to_owned(), json!("/home/someone/cache"));
    }
    resolution
}

#[async_trait::async_trait]
impl pnpm_hooks::CustomFetcher for ScratchWritingFetcher {
    async fn can_fetch(
        &self,
        _id: &str,
        _resolution: serde_json::Value,
    ) -> Result<bool, pnpm_hooks::HookError> {
        Ok(true)
    }
    async fn can_fetch_with_resolution(
        &self,
        _id: &str,
        resolution: serde_json::Value,
    ) -> Result<(bool, serde_json::Value), pnpm_hooks::HookError> {
        Ok((true, with_scratch_field(resolution)))
    }
    async fn fetch(
        &self,
        _id: &str,
        _resolution: serde_json::Value,
        _opts: serde_json::Value,
    ) -> Result<serde_json::Value, pnpm_hooks::HookError> {
        Ok(json!({"delegate": {"tarball": "file:./repo.tgz"}}))
    }
}

#[async_trait::async_trait]
impl pnpm_hooks::CustomFetcher for DecliningScratchWritingFetcher {
    async fn can_fetch(
        &self,
        _id: &str,
        _resolution: serde_json::Value,
    ) -> Result<bool, pnpm_hooks::HookError> {
        Ok(false)
    }
    async fn can_fetch_with_resolution(
        &self,
        _id: &str,
        resolution: serde_json::Value,
    ) -> Result<(bool, serde_json::Value), pnpm_hooks::HookError> {
        Ok((false, with_scratch_field(resolution)))
    }
    async fn fetch(
        &self,
        _id: &str,
        _resolution: serde_json::Value,
        _opts: serde_json::Value,
    ) -> Result<serde_json::Value, pnpm_hooks::HookError> {
        unreachable!("a declining fetcher is never asked to fetch")
    }
}

#[tokio::test]
async fn custom_fetcher_reads_git_subdirectory_without_adding_integrity() {
    let dir = tempdir().unwrap();
    let manifest = json!({"name": "subpackage", "dependencies": {"ms": "2.1.2"}}).to_string();
    let body = pnpm_testing_utils::fixtures::tarball_entries(&[
        ("repo/package.json", br#"{"name":"root"}"#),
        ("repo/packages/foo/package.json", manifest.as_bytes()),
    ]);
    std::fs::write(dir.path().join("repo.tgz"), body).unwrap();
    let mut result = result_without_manifest("subpackage");
    let LockfileResolution::Tarball(tarball) = &mut result.resolution else { panic!("tarball") };
    tarball.tarball =
        "https://codeload.github.com/example/repo/tar.gz/0123456789abcdef0123456789abcdef01234567"
            .to_string();
    tarball.path = Some("/packages/foo".to_string());
    let mut resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: result.clone() }),
        false,
    );
    Arc::get_mut(&mut resolver.ctx).unwrap().policy.custom_session =
        Some(Arc::new(pnpm_deps_restorer::CustomFetcherSession::new(vec![Arc::new(
            LocalArchiveFetcher { path: Some("/packages/foo") },
        )])));
    resolver.populate_missing_tarball_metadata(&mut result, dir.path()).await.unwrap();
    assert_eq!(result.package.manifest.unwrap()["dependencies"]["ms"], json!("2.1.2"));
    assert_eq!(result.resolution.integrity(), None);
    let mut result = result_without_manifest("subpackage");
    result.resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/example/repo/tar.gz/0123456789abcdef0123456789abcdef01234567".to_string(),
        path: Some("/packages/foo".to_string()),
        integrity: None, revision: None, git_hosted: None,
    });
    let mut resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: result.clone() }),
        false,
    );
    Arc::get_mut(&mut resolver.ctx).unwrap().policy.custom_session =
        Some(Arc::new(pnpm_deps_restorer::CustomFetcherSession::new(vec![Arc::new(
            LocalArchiveFetcher { path: None },
        )])));
    resolver.populate_missing_tarball_metadata(&mut result, dir.path()).await.unwrap();
    assert_eq!(result.package.manifest.unwrap()["name"], json!("root"));
}

/// A custom resolution names no archive of its own, so the fetcher that claims
/// it is the only route to the manifest the dependency walk reads the package's
/// children from. <https://github.com/pnpm/pnpm/issues/15552>
#[tokio::test]
async fn custom_fetcher_reads_a_manifest_for_a_custom_resolution() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("repo.tgz"), tarball_with_a_dependency("vendored")).unwrap();
    let mut result = result_without_manifest("vendored");
    // The custom-resolver adapter learns neither a name nor a version from a
    // resolution it does not understand, so the read is named by the resolver's id.
    result.package.name_ver = None;
    let resolution = LockfileResolution::Custom(
        serde_json::from_value(json!({
            "type": "custom:vendored", "name": "vendored", "version": "1.0.0",
        }))
        .unwrap(),
    );
    result.resolution = resolution.clone();
    let mut resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: result.clone() }),
        false,
    );
    Arc::get_mut(&mut resolver.ctx).unwrap().policy.custom_session =
        Some(Arc::new(pnpm_deps_restorer::CustomFetcherSession::new(vec![Arc::new(
            LocalArchiveFetcher { path: None },
        )])));
    resolver.populate_missing_tarball_metadata(&mut result, dir.path()).await.unwrap();
    assert_eq!(result.package.manifest.unwrap()["dependencies"]["ms"], json!("2.1.2"));
    // The fetcher owns what identifies a custom resolution, so the read records
    // neither the archive's hash nor its own URL over it.
    assert_eq!(dbg!(result.resolution), resolution);
}

/// A `canFetch` hook may leave scratch fields on the resolution it is handed.
/// `decode_resolution` drops those only from a resolution with no `type`, and
/// `CustomResolution::extra` accepts any key, so taking the fetcher's copy of a
/// custom resolution would commit a fetcher's private state to the lockfile.
#[tokio::test]
async fn a_custom_resolution_keeps_no_scratch_field_a_fetcher_left_on_it() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("repo.tgz"), tarball_with_a_dependency("vendored")).unwrap();
    let mut result = result_without_manifest("vendored");
    result.package.name_ver = None;
    let resolution = LockfileResolution::Custom(
        serde_json::from_value(json!({"type": "custom:vendored"})).unwrap(),
    );
    result.resolution = resolution.clone();
    let mut resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: result.clone() }),
        false,
    );
    Arc::get_mut(&mut resolver.ctx).unwrap().policy.custom_session = Some(Arc::new(
        pnpm_deps_restorer::CustomFetcherSession::new(vec![Arc::new(ScratchWritingFetcher)]),
    ));
    resolver.populate_missing_tarball_metadata(&mut result, dir.path()).await.unwrap();
    assert_eq!(result.package.manifest.unwrap()["dependencies"]["ms"], json!("2.1.2"));
    assert_eq!(dbg!(result.resolution), resolution);
}

/// Fetchers are configured but none claims the package. The read cannot reach
/// an archive, and the round trip through every `canFetch` must not become the
/// resolution the lockfile records.
#[tokio::test]
async fn a_declined_custom_resolution_is_recorded_as_the_resolver_wrote_it() {
    let dir = tempdir().unwrap();
    let mut result = result_without_manifest("vendored");
    result.package.name_ver = None;
    let resolution = LockfileResolution::Custom(
        serde_json::from_value(json!({"type": "custom:vendored"})).unwrap(),
    );
    result.resolution = resolution.clone();
    let mut resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: result.clone() }),
        false,
    );
    Arc::get_mut(&mut resolver.ctx).unwrap().policy.custom_session =
        Some(Arc::new(pnpm_deps_restorer::CustomFetcherSession::new(vec![Arc::new(
            DecliningScratchWritingFetcher,
        )])));
    resolver.populate_missing_tarball_metadata(&mut result, dir.path()).await.unwrap();
    assert!(result.package.manifest.is_none());
    assert_eq!(dbg!(result.resolution), resolution);
}

/// Without a fetcher to claim it, nothing can read a custom resolution's
/// archive, and a read of the resolution's own shape would fetch the wrong one.
#[tokio::test]
async fn a_custom_resolution_is_left_alone_when_no_fetcher_is_configured() {
    let dir = tempdir().unwrap();
    let mut result = result_without_manifest("vendored");
    result.resolution = LockfileResolution::Custom(
        serde_json::from_value(json!({"type": "custom:vendored"})).unwrap(),
    );
    let resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: result.clone() }),
        false,
    );
    resolver.populate_missing_tarball_metadata(&mut result, dir.path()).await.unwrap();
    assert!(result.package.manifest.is_none());
}

#[tokio::test]
async fn unpinned_git_manifest_recovery_publishes_the_install_cache_key() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("archive.tgz");
    std::fs::write(&path, tarball_with_a_dependency("git-package")).unwrap();
    let source_url = format!("file:{}", path.display());
    let resolver = resolver_with_prefetch(
        dir.path(),
        Box::new(FixedResolver { result: result_without_manifest("git-package") }),
        false,
    );
    let tarball = TarballResolution {
        tarball: "https://codeload.github.com/example/repo/tar.gz/0123456789abcdef0123456789abcdef01234567".to_string(),
        integrity: None, revision: None, git_hosted: None, path: None,
    };
    // Use a local transport fixture for the commit-addressed resolution.
    let metadata = resolver.read_archive(&tarball, &source_url, "git-package@1.0.0").await.unwrap();
    assert_eq!(metadata.resolution.integrity(), None);
    assert_eq!(metadata.manifest.unwrap()["dependencies"]["ms"], json!("2.1.2"));
    std::fs::remove_file(&path).unwrap();
    let files = resolver.ctx
        .tarball_download(&source_url, "git-package@1.0.0", None, None, None)
        .run_with_mem_cache::<pnpm_reporter::SilentReporter>(&resolver.ctx.mem_cache)
        .await
        .unwrap();
    assert!(files.contains_key("package.json"));
}

#[tokio::test]
async fn evicted_archive_cache_entry_does_not_abort_manifest_recovery() {
    let resolver = resolver();
    resolver.share_commit_addressed_archive(
        "https://example.test/archive.tgz",
        &ssri::Integrity::from(b"archive"),
        false,
    );
    assert!(resolver.ctx.mem_cache.is_empty());
}
