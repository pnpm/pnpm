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
    let result = manifestless_tarball_result("file:./package.tgz", &integrity);
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
    assert_eq!(tarball.tarball, "file:./package.tgz");
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
        Ok(json!({"delegate": {"tarball": "file:./repo.tgz", "path": self.path}}))
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
