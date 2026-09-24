use super::{EarlyMaterializer, lock};
use pnpm_config::Config;
use pnpm_lockfile::{LockfileResolution, TarballResolution};
use pnpm_reporter::SilentReporter;
use pnpm_resolving_deps_resolver::FinalizedPackage;
use pnpm_resolving_resolver_base::{ResolveResult, ResolvedPackageInfo};
use pnpm_tarball::{CacheValue, CachedTarball, MemCache, package_mem_cache_key};
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicU8},
};
use tokio::sync::RwLock;

#[tokio::test]
async fn repeated_resolution_rounds_schedule_each_slot_once() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config { virtual_store_dir: dir.path().join(".pnpm"), ..Config::default() };
    let materializer =
        EarlyMaterializer::<SilentReporter>::new(&config, Arc::new(MemCache::default()));
    let package = finalized_package("foo", "1.0.0");
    materializer.schedule(&package);
    materializer.schedule(&package);
    materializer.schedule(&package);
    assert_eq!(lock(&materializer.tasks).len(), 1);
    assert_eq!(lock(&materializer.slots).len(), 1);
    assert_eq!(materializer.finish(|_| true, &AtomicU8::new(0)).await, 0);
}

/// A package with a build ahead of it is left to the link phase, which
/// imports it without sharing inodes with the store.
#[tokio::test]
async fn a_package_that_requires_a_build_is_left_to_the_link_phase() {
    let dir = tempfile::tempdir().unwrap();
    let virtual_store_dir = dir.path().join(".pnpm");
    let config = Config { virtual_store_dir: virtual_store_dir.clone(), ..Config::default() };
    let mem_cache = Arc::new(MemCache::default());
    for (name, manifest) in [
        ("built", r#"{"name":"built","version":"1.0.0","scripts":{"postinstall":"node x.js"}}"#),
        ("plain", r#"{"name":"plain","version":"1.0.0"}"#),
    ] {
        let package_json = dir.path().join(format!("{name}.json"));
        std::fs::write(&package_json, manifest).unwrap();
        let files = HashMap::from([("package.json".to_string(), package_json)]);
        let package = finalized_package(name, "1.0.0");
        let LockfileResolution::Tarball(tarball) = &package.result.resolution else {
            unreachable!()
        };
        mem_cache.insert(
            package_mem_cache_key(&tarball.tarball, tarball.integrity.as_ref(), false),
            Arc::new(RwLock::new(CacheValue::Available(CachedTarball::from_files(files)))),
        );
    }
    let materializer = EarlyMaterializer::<SilentReporter>::new(&config, mem_cache);
    materializer.schedule(&finalized_package("built", "1.0.0"));
    materializer.schedule(&finalized_package("plain", "1.0.0"));
    // Let both tasks import before `finish` closes the materializer.
    let mut tasks = std::mem::take(&mut *lock(&materializer.tasks));
    while tasks.join_next().await.is_some() {}

    assert_eq!(materializer.finish(|_| true, &AtomicU8::new(0)).await, 1);
    assert!(virtual_store_dir.join("plain@1.0.0/node_modules/plain/package.json").is_file());
    assert!(!virtual_store_dir.join("built@1.0.0/node_modules/built").exists());
}

fn finalized_package(name: &str, version: &str) -> FinalizedPackage {
    let pkg_id = format!("{name}@{version}");
    FinalizedPackage {
        pkg_id: pkg_id.as_str().into(),
        result: Arc::new(ResolveResult {
            id: pkg_id.as_str().into(),
            resolution: LockfileResolution::Tarball(TarballResolution {
                integrity: Some("sha512-AAAA".parse().unwrap()),
                tarball: format!("https://registry.example/{name}.tgz"),
                revision: None,
                git_hosted: None,
                path: None,
            }),
            resolved_via: "npm-registry".into(),
            normalized_bare_specifier: None,
            alias: None,
            policy_violation: None,
            package: ResolvedPackageInfo {
                name_ver: Some(pkg_id.parse().unwrap()),
                latest: None,
                published_at: None,
                manifest: None,
                non_deprecated_alternative: None,
            },
        }),
        children: Vec::new(),
    }
}
