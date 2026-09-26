use super::{append_workspace_yaml_key, is_absent, read_wanted_lockfile, write_manifest};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, process::Command};

/// TS: `not installing optional dependencies when optional is false`
/// (`optionalDependencies.ts:391`). The root's own optional is dropped,
/// the regular dependency installs with its regular subdependency, and
/// its transitive optional is dropped too.
#[test]
fn not_installing_optional_dependencies_when_optional_is_false() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-good-optional": "*" },
            "optionalDependencies": { "is-positive": "1.0.0" },
        }),
    );

    pacquet
        .with_args(["install", "--no-optional"])
        .assert()
        .success();

    assert!(!workspace.join("node_modules/is-positive").exists());
    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-good-optional/package.json").exists());
    let good_optional_modules = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/@pnpm.e2e",
    );
    assert!(
        good_optional_modules.join("dep-of-pkg-with-1-dep/package.json").exists(),
        "the regular subdependency must be installed",
    );
    assert!(
        !workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive"
            )
            .exists(),
        "the transitive optional must not be linked",
    );

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn optional_setting_excludes_optional_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
            "optionalDependencies": { "@pnpm.e2e/pkg-with-optional": "1.0.0" },
        }),
    );
    append_workspace_yaml_key(&workspace, "optional", "false");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert!(workspace.join("node_modules/is-positive/package.json").exists());
    assert!(is_absent(&workspace.join("node_modules/@pnpm.e2e/pkg-with-optional")));

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--optional"])
        .assert()
        .success();
    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-optional/package.json").exists());

    drop((root, npmrc_info));
}

/// TS: `dependency that is both optional and non-optional is installed,
/// when optional dependencies should be skipped`
/// (`optionalDependencies.ts:712`, pnpm/pnpm issue 8066). Registry-mock
/// fixtures stand in for upstream's `@babel/cli` + `del` pair: the package
/// is a direct regular dependency *and* another dependency's optional.
#[test]
fn both_optional_and_non_optional_dependency_is_installed_when_optionals_are_skipped() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
                "@pnpm.e2e/pkg-with-good-optional": "*",
            },
        }),
    );

    pacquet
        .with_args(["install", "--no-optional"])
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/.pnpm/is-positive@1.0.0").exists(),
        "a package that is also a regular dependency must be materialized",
    );
    assert!(workspace.join("node_modules/is-positive/package.json").exists());

    drop((root, npmrc_info)); // cleanup
}

/// Regression test for pnpm/pnpm#14729: every command that resolves from
/// scratch has to honour `ignoredOptionalDependencies`, not just the
/// lockfile rewrite that absorbs a widened pattern list.
#[test]
fn fresh_resolution_ignores_optional_dependencies_by_name() {
    check_fresh_resolution_ignores_optional_dependencies("[is-positive]");
}

#[test]
fn fresh_resolution_ignores_optional_dependencies_by_pattern() {
    check_fresh_resolution_ignores_optional_dependencies("['is-*', '!is-negative']");
}

fn check_fresh_resolution_ignores_optional_dependencies(patterns: &str) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "ignoredOptionalDependencies", patterns);
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0",
                "is-positive": "1.0.0"
            },
            "optionalDependencies": {
                "is-positive": "1.0.0",
                "is-negative": "1.0.0"
            }
        }),
    );

    for args in [
        vec!["install", "--lockfile-only"],
        vec!["install", "--frozen-lockfile"],
        vec!["dedupe"],
        vec!["add", "is-odd@1.0.0", "--lockfile-only"],
    ] {
        Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_args(&args)
            .assert()
            .success();
        let text = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).unwrap();
        assert!(!text.contains("is-positive@"), "ignored package after {args:?}:\n{text}");
        let lockfile = read_wanted_lockfile(&workspace);
        assert_eq!(
            lockfile.ignored_optional_dependencies,
            Some(serde_saphyr::from_str::<Vec<String>>(patterns).unwrap()),
        );
        let importer = &lockfile.importers["."];
        let ignored: PkgName = "is-positive".parse().unwrap();
        assert!(
            importer.dependencies
                .as_ref()
                .is_none_or(|deps| !deps.contains_key(&ignored)),
            "ignored duplicate regular dependency: {importer:?}",
        );
        assert!(
            importer.optional_dependencies
                .as_ref()
                .is_some_and(|deps| !deps.contains_key(&ignored)
                    && deps.contains_key(&"is-negative".parse().unwrap())),
            "retained optional dependencies: {importer:?}",
        );
        assert!(
            is_absent(&workspace.join("node_modules/is-positive")),
            "ignored direct dependency was linked after {args:?}",
        );
        assert!(
            is_absent(&workspace.join(
                "node_modules/@pnpm.e2e/pkg-with-good-optional/node_modules/is-positive"
            )),
            "ignored transitive dependency was linked after {args:?}",
        );
    }
    drop((root, npmrc_info));
}

/// A package that is both a required dependency of the root and an ignored
/// optional of a dependency still installs. `ignoredOptionalDependencies`
/// drops optional declarations, not the package.
#[test]
fn ignored_optional_dependencies_preserve_required_occurrences() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "ignoredOptionalDependencies", "[is-positive]");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0",
                "is-positive": "1.0.0"
            }
        }),
    );
    pacquet
        .with_arg("install")
        .assert()
        .success();
    let lockfile = read_wanted_lockfile(&workspace);
    let name: PkgName = "is-positive".parse().unwrap();
    assert_eq!(
        lockfile.importers["."].dependencies.as_ref().unwrap()[&name].version.to_string(),
        "1.0.0",
    );
    let parent = &lockfile.snapshots.as_ref().unwrap()
        [&"@pnpm.e2e/pkg-with-good-optional@1.0.0".parse().unwrap()];
    assert!(
        parent.optional_dependencies
            .as_ref()
            .is_none_or(|deps| !deps.contains_key(&name)),
        "ignored optional edge: {parent:?}",
    );
    assert!(
        workspace.join("node_modules/is-positive/package.json").exists(),
        "required occurrence must be installed",
    );
    drop((root, npmrc_info));
}

/// `ignoredOptionalDependencies` runs last in the read-package hook chain,
/// so it also removes optional dependencies that `packageExtensions` and the
/// pnpmfile's `readPackage` introduced rather than only the ones the package
/// publishes.
#[test]
fn ignored_optional_dependencies_apply_after_manifest_hooks() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "ignoredOptionalDependencies", "[is-negative, is-odd]");
    append_workspace_yaml_key(
        &workspace,
        "packageExtensions",
        "\n  '@pnpm.e2e/pkg-with-good-optional':\n    optionalDependencies:\n      is-negative: 1.0.0",
    );
    fs::write(
        workspace.join(".pnpmfile.mjs"),
        r"
export const hooks = {
  readPackage(pkg) {
    if (pkg.name === '@pnpm.e2e/pkg-with-good-optional') {
      return { ...pkg, optionalDependencies: { ...pkg.optionalDependencies, 'is-odd': '1.0.0' } }
    }
    return pkg
  }
}
",
    )
    .unwrap();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-good-optional": "1.0.0" }
        }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let text = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).unwrap();
    assert!(!text.contains("is-negative@"), "packageExtensions optional was kept:\n{text}");
    assert!(!text.contains("is-odd@"), "readPackage optional was kept:\n{text}");
    assert!(
        text.contains("is-positive@"),
        "an optional dependency no pattern matches must survive:\n{text}",
    );
    drop((root, npmrc_info));
}
