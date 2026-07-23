use super::{AddDependencyOptions, apply_allow_build};
use pacquet_config::Config;
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn allow_build_merges_into_config_and_persists_to_workspace_yaml() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config = Config::default();
    apply_allow_build(&mut config, &["esbuild".to_string()], dir.path())
        .expect("allow-build applies");

    // Enabled for the current install.
    assert_eq!(config.allow_builds.get("esbuild"), Some(&true));

    // Persisted to the settings dir's pnpm-workspace.yaml.
    let yaml = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml"))
        .expect("pnpm-workspace.yaml written");
    assert!(yaml.contains("esbuild"), "allowBuilds entry written, got:\n{yaml}");
}

#[test]
fn allow_build_rejects_a_package_the_root_disallows() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config = Config::default();
    config.allow_builds.insert("esbuild".to_string(), false);

    let err = apply_allow_build(&mut config, &["esbuild".to_string()], dir.path())
        .expect_err("disallowed package is rejected");
    assert_eq!(
        err.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_OVERRIDING_IGNORED_BUILT_DEPENDENCIES"),
    );
    // Nothing was persisted for a rejected apply.
    assert!(!dir.path().join("pnpm-workspace.yaml").exists());
}

#[test]
fn allow_build_is_a_noop_when_empty() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config = Config::default();
    apply_allow_build(&mut config, &[], dir.path()).expect("empty allow-build is a no-op");
    assert!(config.allow_builds.is_empty());
    assert!(!dir.path().join("pnpm-workspace.yaml").exists());
}

#[test]
fn dependency_options_to_dependency_groups() {
    use DependencyGroup::{Dev, Optional, Peer, Prod};
    let create_list = |opts: AddDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false,
            save_peer: false
        }),
        [Prod],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_peer: false
        }),
        [Prod],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_peer: false
        }),
        [Dev],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true,
            save_peer: false
        }),
        [Optional],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false,
            save_peer: true
        }),
        [Dev, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_peer: true
        }),
        [Prod, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_peer: true
        }),
        [Dev, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true,
            save_peer: true
        }),
        [Optional, Peer],
    );
}
