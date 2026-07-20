use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_modules_yaml::{Host as ModulesHost, read_modules_manifest, write_modules_manifest};
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fs, process::Command};

#[test]
fn frozen_reinstall_writes_modules_manifest_current_lockfile_and_bins() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/hello-world-js-bin":"1.0.0"}}"#,
    )
    .expect("write manifest");
    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/.modules.yaml").exists());
    assert!(workspace.join("node_modules/.pnpm/lock.yaml").exists());
    assert!(workspace.join("node_modules/.bin/hello-world-js-bin").exists());

    drop((root, mock_instance));
}

/// TS: `installing with no symlinks with PnP`
/// (`deps-installer/test/install/misc.ts:1433`).
#[test]
fn pnp_install_without_symlinks_still_writes_modules_manifest_and_bin_directory() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "project",
            "version": "1.0.0",
            "imports": { "#x": "./x.cjs" },
            "dependencies": {
                "@pnpm.e2e/hello-world-js-bin": "1.0.0",
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join("x.cjs"), "module.exports = 42\n").expect("write imports target");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: pnp\nsymlink: false\n");
    fs::write(&yaml_path, yaml).expect("enable PnP without symlinks");

    pacquet.with_arg("install").assert().success();

    let modules_dir = workspace.join("node_modules");
    assert!(modules_dir.join(".bin/hello-world-js-bin").exists());
    assert!(modules_dir.join(".modules.yaml").exists());
    assert!(modules_dir.join(".pnpm/lock.yaml").exists());
    assert!(workspace.join(".pnp.cjs").exists());
    assert!(
        !modules_dir.join("@pnpm.e2e/hello-world-js-bin").exists(),
        "symlink:false must not create an importer dependency link",
    );

    Command::new("node")
        .with_current_dir(&workspace)
        .with_args(["--require", "./.pnp.cjs", "--eval", "require('@pnpm.e2e/pkg-with-1-dep')"])
        .assert()
        .success();
    Command::new("node")
        .with_current_dir(&workspace)
        .with_args([
            "--require",
            "./.pnp.cjs",
            "--eval",
            "const api = require('module').findPnpApi(); const resolved = api.resolveRequest('#x', __filename); if (require(resolved) !== 42) process.exit(1)",
        ])
        .assert()
        .success();
    let undeclared = Command::new("node")
        .with_current_dir(&workspace)
        .with_args([
            "--require",
            "./.pnp.cjs",
            "--eval",
            "require('@pnpm.e2e/dep-of-pkg-with-1-dep')",
        ])
        .output()
        .expect("run undeclared PnP require");
    assert!(!undeclared.status.success());
    assert!(
        String::from_utf8_lossy(&undeclared.stderr).contains("isn't declared in your dependencies"),
    );

    fs::remove_file(workspace.join(".pnp.cjs")).expect("remove PnP loader");
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(workspace.join(".pnp.cjs").exists());

    drop((root, mock_instance));
}

/// TS: `installing with publicHoistPattern=* in a project with external
/// lockfile` (`deps-restorer/test/index.ts:690`).
#[test]
fn public_hoist_uses_the_project_root_when_the_lockfile_is_external() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let project = workspace.join("pkg");
    fs::create_dir_all(&project).expect("create project");
    fs::write(workspace.join("package.json"), r#"{"private":true}"#)
        .expect("write root package.json");
    fs::write(
        project.join("package.json"),
        serde_json::json!({
            "name": "external-lockfile-project",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write project package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("packages:\n  - pkg\npublicHoistPattern:\n  - '*'\n");
    fs::write(&yaml_path, yaml).expect("configure workspace public hoist");

    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&project)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["--filter", "external-lockfile-project", "install", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json").exists(),
        "the public hoist must be anchored at the lockfile/project root",
    );
    assert!(
        project.join("node_modules/@pnpm.e2e/pkg-with-1-dep/package.json").exists(),
        "the selected project must keep its direct dependency link",
    );

    drop((root, mock_instance));
}

/// TS: `the modules cache is pruned when it expires and headless install
/// is used` (`deps-installer/test/install/modulesCache.ts:52`).
#[test]
fn expired_modules_cache_is_pruned_during_frozen_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0",
                "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("modulesCacheMaxAge: 2\noptimisticRepeatInstall: false\n");
    fs::write(&yaml_path, yaml).expect("configure modules cache");

    pacquet.with_arg("install").assert().success();
    let stale_slot = workspace.join("node_modules/.pnpm/@pnpm.e2e+foo@100.0.0");
    assert!(stale_slot.exists());

    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("remove dependency from package.json");
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(stale_slot.exists(), "an unexpired cache entry must be retained");
    let current_lockfile =
        fs::read_to_string(workspace.join("node_modules/.pnpm/lock.yaml")).expect("read current");
    assert!(!current_lockfile.contains("@pnpm.e2e/foo"));

    let modules_dir = workspace.join("node_modules");
    let mut modules = read_modules_manifest::<ModulesHost>(&modules_dir)
        .expect("read modules manifest")
        .expect("modules manifest exists");
    modules.pruned_at = "Thu, 01 Jan 1970 00:00:00 GMT".to_owned();
    write_modules_manifest::<ModulesHost>(&modules_dir, modules)
        .expect("expire modules cache timestamp");
    assert_eq!(
        read_modules_manifest::<ModulesHost>(&modules_dir)
            .expect("reread modules manifest")
            .expect("modules manifest exists")
            .pruned_at,
        "Thu, 01 Jan 1970 00:00:00 GMT",
    );

    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let entries = fs::read_dir(workspace.join("node_modules/.pnpm"))
        .expect("read virtual store")
        .map(|entry| entry.expect("read entry").file_name())
        .collect::<Vec<_>>();
    assert!(!stale_slot.exists(), "the expired orphaned slot must be pruned: {entries:?}");

    drop((root, mock_instance));
}

/// TS: `rewrites node_modules created by npm`
/// (`deps-installer/test/install/misc.ts:1087`).
#[test]
fn rewrites_node_modules_created_by_npm() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let npm_dep = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    fs::create_dir_all(&npm_dep).expect("create npm-style dependency directory");
    fs::write(
        npm_dep.join("package.json"),
        r#"{"name":"@pnpm.e2e/hello-world-js-bin","version":"0.0.0"}"#,
    )
    .expect("write npm-style package");
    fs::write(workspace.join("package-lock.json"), r#"{"lockfileVersion":3}"#)
        .expect("write npm lockfile");

    pacquet.with_arg("install").assert().success();
    assert!(is_symlink_or_junction(&npm_dep).expect("inspect installed dependency"));
    assert!(workspace.join("node_modules/.bin/hello-world-js-bin").exists());

    drop((root, mock_instance));
}
