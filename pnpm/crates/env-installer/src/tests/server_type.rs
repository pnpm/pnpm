use super::{
    BTreeMap, EnvLockfile, Harness, LockfileResolution, PackageKey, SilentReporter, TempDir,
    TestRegistry, build_resolver, clean_spec, harness_for, options,
    resolve_and_install_config_deps,
};
use pnpm_lockfile::{RegistryOptions, RegistryServerType};

fn pnpr_harness(registry: &TestRegistry) -> Harness {
    let mut harness = harness_for(registry.url().to_string());
    harness.registry_options_by_url = BTreeMap::from([(
        registry.url().to_string(),
        RegistryOptions { server_type: Some(RegistryServerType::Pnpr), ..Default::default() },
    )]);
    harness
}

#[tokio::test]
async fn frozen_install_fetches_a_config_dep_from_the_declared_pnpr_layout() {
    let registry_root = TempDir::new().unwrap();
    let registry = TestRegistry::start_recording_with_own_storage(registry_root.path(), &[]);
    let (resolver, _cache) = build_resolver(registry.url());
    let root = TempDir::new().unwrap();
    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));

    let recording_harness = pnpr_harness(&registry);
    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&recording_harness, root.path(), false),
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let resolution = &env.packages[&key].resolution;
    assert!(
        matches!(resolution, LockfileResolution::Registry(_)),
        "unexpected resolution: {resolution:?}",
    );

    std::fs::remove_dir_all(root.path().join("node_modules")).unwrap();
    registry.take_requests();
    let empty_store_harness = pnpr_harness(&registry);
    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&empty_store_harness, root.path(), true),
    )
    .await
    .unwrap();

    let requests: Vec<String> = registry
        .take_requests()
        .into_iter()
        .map(|request| request.path)
        .collect();
    dbg!(&requests);
    let fetched_from_pnpr_layout = requests
        .iter()
        .any(|path| path.ends_with("/@pnpm.e2e/foo@100.0.0"));
    assert!(fetched_from_pnpr_layout, "no pnpr-layout tarball request");
    let fetched_from_npm_layout = requests
        .iter()
        .any(|path| path.ends_with(".tgz"));
    assert!(!fetched_from_npm_layout, "fetched an npm-layout tarball");
}
