use super::{AddDependencyOptions, AddError, apply_allow_build, workspace_selectors};
use crate::cargo_manifest::CargoDependencyKind;
use pnpm_config::Config;
use pnpm_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn allow_build_merges_into_config_and_persists_to_workspace_yaml() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config = Config::default();
    apply_allow_build(&mut config, &["esbuild".to_string()], dir.path())
        .expect("allow-build applies");

    assert_eq!(config.allow_builds.get("esbuild"), Some(&true), "enabled for this install");

    let yaml = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml"))
        .expect("pnpm-workspace.yaml written");
    assert!(yaml.contains("esbuild: true"), "allowBuilds entry persisted, got:\n{yaml}");
}

#[test]
fn allow_build_negation_sets_false_in_config_and_workspace_yaml() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config = Config::default();
    apply_allow_build(&mut config, &["!core-js".to_string()], dir.path())
        .expect("allow-build negation applies");

    assert_eq!(config.allow_builds.get("core-js"), Some(&false), "disabled for this install");

    let yaml = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml"))
        .expect("pnpm-workspace.yaml written");
    assert!(yaml.contains("core-js: false"), "allowBuilds entry persisted, got:\n{yaml}");
}

#[test]
fn allow_build_rejects_an_argument_that_names_no_package() {
    for allow_build in [&["!".to_string()][..], &[String::new()][..]] {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut config = Config::default();
        let err = apply_allow_build(&mut config, allow_build, dir.path())
            .expect_err("an empty package name is rejected");
        assert_eq!(
            err.code().map(|code| code.to_string()).as_deref(),
            Some("ERR_PNPM_ALLOW_BUILD_MISSING_PACKAGE"),
        );
        assert!(config.allow_builds.is_empty());
        assert!(
            !dir.path().join("pnpm-workspace.yaml").exists(),
            "a rejected apply persists nothing",
        );
    }
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
    assert!(!dir.path().join("pnpm-workspace.yaml").exists(), "a rejected apply persists nothing");
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
            save_build: false,
            save_peer: false,
            no_save_peer: false,
        }),
        [Prod],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_build: false,
            save_peer: false,
            no_save_peer: false,
        }),
        [Prod],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_build: false,
            save_peer: false,
            no_save_peer: false,
        }),
        [Dev],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true,
            save_build: false,
            save_peer: false,
            no_save_peer: false,
        }),
        [Optional],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false,
            save_build: false,
            save_peer: true,
            no_save_peer: false,
        }),
        [Dev, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_build: false,
            save_peer: true,
            no_save_peer: false,
        }),
        [Prod, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_build: false,
            save_peer: true,
            no_save_peer: false,
        }),
        [Dev, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true,
            save_build: false,
            save_peer: true,
            no_save_peer: false,
        }),
        [Optional, Peer],
    );
}

#[test]
fn save_build_selects_only_the_cargo_build_table() {
    let options = AddDependencyOptions {
        save_prod: false,
        save_dev: false,
        save_optional: false,
        save_build: true,
        save_peer: false,
        no_save_peer: false,
    };

    assert_eq!(options.cargo_dependency_kind(false).unwrap(), CargoDependencyKind::Build);
    assert_eq!(options.dependency_groups().collect::<Vec<_>>(), []);
}

#[test]
fn save_build_rejects_mixed_packages_and_conflicting_cargo_targets() {
    let save_build = AddDependencyOptions {
        save_prod: false,
        save_dev: false,
        save_optional: false,
        save_build: true,
        save_peer: false,
        no_save_peer: false,
    };
    assert!(save_build.cargo_dependency_kind(true).is_err());

    for options in [
        AddDependencyOptions { save_prod: true, ..save_build.clone() },
        AddDependencyOptions { save_dev: true, ..save_build },
    ] {
        assert!(options.cargo_dependency_kind(false).is_err());
    }
}

fn workspace_packages(names: &[&str]) -> pnpm_resolving_resolver_base::WorkspacePackages {
    names.iter().map(|name| (name.to_string(), std::collections::BTreeMap::default())).collect()
}

#[test]
fn workspace_selectors_request_the_workspace_copy_of_each_package() {
    let packages = workspace_packages(&["foo", "@scope/bar", "baz"]);
    let selectors = ["foo", "@scope/bar@^1.0.0", "baz@workspace:~"].map(str::to_string);

    let rewritten = workspace_selectors(&selectors, &packages).expect("every selector rewrites");

    assert_eq!(rewritten, ["foo@workspace:*", "@scope/bar@workspace:^1.0.0", "baz@workspace:~"]);
}

#[test]
fn workspace_selectors_reject_a_package_outside_the_workspace() {
    let packages = workspace_packages(&["foo"]);

    let err = workspace_selectors(&["foo".to_string(), "bar@1".to_string()], &packages)
        .expect_err("bar is not in the workspace");

    assert!(matches!(&err, AddError::WorkspacePackageNotFound { name } if name == "bar"), "{err}");
}

#[test]
fn workspace_selectors_reject_a_selector_without_a_package_name() {
    let packages = workspace_packages(&["foo"]);

    let err = workspace_selectors(&["./local-dir".to_string()], &packages)
        .expect_err("a path carries no package name");

    assert!(
        matches!(&err, AddError::NoPkgNameInSpec { selector } if selector == "./local-dir"),
        "{err}",
    );
}
