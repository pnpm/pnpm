use super::{
    BTreeMap, ConfigDepError, ConfigDependency, ConfigDepsInstallOptions, EnvLockfile, Harness,
    LockfileResolution, PackageKey, SilentReporter, TempDir, build_resolver, clean_spec, harness,
    integrity_of, options, resolve_and_install_config_deps,
};
use std::path::Path;

/// Every fixture version is published after this cutoff.
fn options_with_release_age_cutoff<'a>(
    harness: &'a Harness,
    root_dir: &'a Path,
) -> ConfigDepsInstallOptions<'a> {
    let mut opts = options(harness, root_dir, false);
    opts.verification.resolution_policy.published_by =
        Some("2000-01-01T00:00:00Z".parse().expect("parse cutoff"));
    opts
}

#[tokio::test]
async fn rejects_a_config_dep_newer_than_the_release_age_cutoff() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();
    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));

    let error = resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options_with_release_age_cutoff(&harness, root.path()),
    )
    .await
    .expect_err("the version is newer than the cutoff");

    dbg!(&error);
    assert!(
        matches!(&error, ConfigDepError::BadConfigDep { message } if message.contains("minimumReleaseAge")),
    );
}

/// Verification skips `version+integrity` pins, so resolution does too.
#[tokio::test]
async fn resolves_a_pinned_config_dep_newer_than_the_release_age_cutoff() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();
    let integrity = integrity_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await;
    let mut config_deps = BTreeMap::new();
    config_deps.insert(
        "@pnpm.e2e/foo".to_string(),
        ConfigDependency::VersionWithIntegrity(format!("100.0.0+{integrity}")),
    );

    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options_with_release_age_cutoff(&harness, root.path()),
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let resolution = &env.packages[&key].resolution;
    dbg!(resolution);
    assert!(matches!(
        resolution,
        LockfileResolution::Registry(registry) if registry.integrity.to_string() == integrity,
    ));
    assert!(
        root.path()
            .join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json")
            .exists(),
        "the pinned config dependency is installed",
    );
}
