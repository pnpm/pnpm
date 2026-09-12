use super::{
    BTreeMap, ConfigDependency, EnvLockfile, HashMap, LockfileResolution, PackageKey,
    SilentReporter, TempDir, build_resolver, harness, integrity_of, options,
    resolve_and_install_config_deps, tarball_url_of,
};

/// Reaching the registry under a different host spelling than the one it
/// advertises its tarballs on stands in for GitLab's npm registry, which
/// serves tarballs from a project endpoint the group endpoint cannot
/// derive. See <https://github.com/pnpm/pnpm/issues/13765>.
#[tokio::test]
async fn takes_old_format_tarball_url_from_the_packument() {
    let harness = harness();
    let aliased_registry = harness.registry_url.replace("127.0.0.1", "localhost");
    let (resolver, _cache) = build_resolver(&aliased_registry);
    let root = TempDir::new().unwrap();

    let integrity = integrity_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await;
    let advertised_tarball = tarball_url_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await;
    let mut config_deps = BTreeMap::new();
    config_deps.insert(
        "@pnpm.e2e/foo".to_string(),
        ConfigDependency::VersionWithIntegrity(format!("100.0.0+{integrity}")),
    );

    let registries = HashMap::from([("default".to_string(), aliased_registry)]);
    let mut opts = options(&harness, root.path(), false);
    opts.registries = &registries;

    resolve_and_install_config_deps::<SilentReporter>(&config_deps, &resolver, &opts)
        .await
        .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let resolution = &env.packages[&key].resolution;
    dbg!(resolution);
    let LockfileResolution::Tarball(tarball) = resolution else {
        panic!("expected the tarball URL the packument advertises to be recorded");
    };
    assert_eq!(tarball.tarball, advertised_tarball);
    assert_eq!(tarball.integrity.as_ref().unwrap().to_string(), integrity);
}
