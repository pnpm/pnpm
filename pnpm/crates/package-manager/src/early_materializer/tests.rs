use super::{EarlyMaterializer, lock};
use pnpm_config::Config;
use pnpm_lockfile::{LockfileResolution, TarballResolution};
use pnpm_reporter::SilentReporter;
use pnpm_resolving_deps_resolver::FinalizedPackage;
use pnpm_resolving_resolver_base::{ResolveResult, ResolvedPackageInfo};
use pnpm_tarball::MemCache;
use std::sync::{Arc, atomic::AtomicU8};

#[tokio::test]
async fn repeated_resolution_rounds_schedule_each_slot_once() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config { virtual_store_dir: dir.path().join(".pnpm"), ..Config::default() };
    let materializer =
        EarlyMaterializer::<SilentReporter>::new(&config, Arc::new(MemCache::default()));
    let package = FinalizedPackage {
        pkg_id: "foo@1.0.0".into(),
        result: Arc::new(ResolveResult {
            id: "foo@1.0.0".into(),
            resolution: LockfileResolution::Tarball(TarballResolution {
                integrity: Some("sha512-AAAA".parse().unwrap()),
                tarball: "https://registry.example/foo.tgz".into(),
                revision: None,
                git_hosted: None,
                path: None,
            }),
            resolved_via: "npm-registry".into(),
            normalized_bare_specifier: None,
            alias: None,
            policy_violation: None,
            package: ResolvedPackageInfo {
                requested_name: None,
                name_ver: Some("foo@1.0.0".parse().unwrap()),
                latest: None,
                published_at: None,
                manifest: None,
                non_deprecated_alternative: None,
            },
        }),
        children: Vec::new(),
    };
    materializer.schedule(&package);
    materializer.schedule(&package);
    materializer.schedule(&package);
    assert_eq!(lock(&materializer.tasks).len(), 1);
    assert_eq!(lock(&materializer.slots).len(), 1);
    assert_eq!(materializer.finish(|_| true, &AtomicU8::new(0)).await, 0);
}
