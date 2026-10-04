use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn assert_no_node_modules(root: &Path) {
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.expect("walk application");
        assert_ne!(entry.file_name(), "node_modules", "{}", entry.path().display());
    }
}

#[test]
fn cas_install_runs_scripts_bins_and_children_without_node_modules() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "application",
            "scripts": { "test": "node app.cjs" },
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
            "devDependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" }
        })
        .to_string(),
    )
    .unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker: cas\n");
    fs::write(yaml_path, yaml).unwrap();
    fs::write(workspace.join("app.cjs"), "const assert = require('node:assert/strict'); const value = require('@pnpm.e2e/pkg-with-1-dep')(); assert.equal(value.name, '@pnpm.e2e/dep-of-pkg-with-1-dep'); require('node:child_process').execFileSync(process.execPath, ['-e', `require('@pnpm.e2e/pkg-with-1-dep')()`], {stdio: 'inherit'}); console.log('CAS works');").unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_no_node_modules(&workspace);
    #[cfg(unix)]
    Command::new(workspace.join(".pnpm/.bin/hello-world-js-bin"))
        .with_current_dir(&workspace)
        .env_remove("NODE_OPTIONS")
        .assert()
        .success();
    for args in [vec!["run", "test"], vec!["exec", "node", "app.cjs"], vec!["node", "app.cjs"]] {
        let output = Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_args(args)
            .assert()
            .success();
        assert!(String::from_utf8_lossy(&output.get_output().stdout).contains("CAS works"));
    }
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "hello-world-js-bin"])
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_no_node_modules(&workspace);
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["run", "test"])
        .assert()
        .success();
    let manifest: Value =
        serde_json::from_slice(&fs::read(workspace.join(".pnpm-store.json")).unwrap()).unwrap();
    assert!(manifest["packages"]["@pnpm.e2e/pkg-with-1-dep@100.0.0"]["files"].is_object());
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["install", "--prod", "--frozen-lockfile"])
        .assert()
        .success();
    let production: Value =
        serde_json::from_slice(&fs::read(workspace.join(".pnpm-store.json")).unwrap()).unwrap();
    assert!(production["packages"]["."]["dependencies"]["@pnpm.e2e/hello-world-js-bin"].is_null());
    assert!(!workspace.join(".pnpm/.bin/hello-world-js-bin").exists());
    assert_no_node_modules(&workspace);
    drop((root, mock_instance));
}

#[test]
fn cas_opt_out_materializes_complete_dependency_tree_in_gvs() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0","@pnpm.e2e/hello-world-js-bin":"1.0.0"}}"#).unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker: cas\ncasMaterialize:\n  - '@pnpm.e2e/pkg-with-1-dep'\n");
    fs::write(yaml_path, yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_no_node_modules(&workspace);
    let manifest: Value =
        serde_json::from_slice(&fs::read(workspace.join(".pnpm-store.json")).unwrap()).unwrap();
    let packages = manifest["packages"].as_object().unwrap();
    let materialized: Vec<_> = packages
        .values()
        .filter(|package| package["resolution"] == "node")
        .collect();
    assert_eq!(materialized.len(), 2);
    for package in materialized {
        let path = Path::new(package["root"].as_str().unwrap());
        assert!(path.join("package.json").is_file());
        assert!(!path.starts_with(&workspace));
    }
    assert!(packages["@pnpm.e2e/hello-world-js-bin@1.0.0"]["files"].is_object());
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["store", "prune"])
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "hello-world-js-bin"])
        .assert()
        .success();

    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args([
            "exec",
            "node",
            "-e",
            "const p=require('@pnpm.e2e/pkg-with-1-dep'); console.log(p().name)",
        ])
        .assert()
        .success();
    drop((root, mock_instance));
}

#[test]
fn cas_workspace_preserves_workspace_and_alias_dependency_contexts() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::create_dir_all(workspace.join("packages/child")).unwrap();
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"root","dependencies":{"child":"workspace:*"}}"#,
    )
    .unwrap();
    fs::write(workspace.join("packages/child/package.json"), r#"{"name":"child","version":"1.0.0","main":"index.cjs","scripts":{"test":"node index.cjs"},"dependencies":{"aliased":"npm:@pnpm.e2e/pkg-with-1-dep@100.0.0"}}"#).unwrap();
    fs::write(workspace.join("packages/child/index.cjs"), "require('node:assert/strict').equal(require('aliased')().name, '@pnpm.e2e/dep-of-pkg-with-1-dep'); module.exports = 42;").unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker: cas\npackages:\n  - packages/*\n");
    fs::write(yaml_path, yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args([
            "exec",
            "node",
            "-e",
            "require('node:assert/strict').equal(require('child'),42)",
        ])
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["--filter", "child", "test"])
        .assert()
        .success();
    assert_no_node_modules(&workspace);
    drop((root, mock_instance));
}

#[test]
fn frozen_cas_install_applies_opt_out_changes_and_repairs_missing_manifest() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker: cas\n");
    fs::write(&yaml_path, &yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    for materialize in [true, false] {
        let config = if materialize {
            format!("{yaml}casMaterialize:\n  - '@pnpm.e2e/pkg-with-1-dep'\n")
        } else {
            yaml.clone()
        };
        fs::write(&yaml_path, config).unwrap();
        fs::remove_file(workspace.join(".pnpm-store.json")).unwrap();
        Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();
        let manifest: Value =
            serde_json::from_slice(&fs::read(workspace.join(".pnpm-store.json")).unwrap()).unwrap();
        assert_eq!(
            manifest["packages"]["@pnpm.e2e/pkg-with-1-dep@100.0.0"]["resolution"] == "node",
            materialize,
        );
        Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_args(["exec", "node", "-e", "require('@pnpm.e2e/pkg-with-1-dep')()"])
            .assert()
            .success();
    }
    assert_no_node_modules(&workspace);
    drop((root, mock_instance));
}

#[test]
fn cas_requires_opt_out_for_builds_and_links_generated_bins_after_building() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/has-bin-and-needs-build":"1.0.0"}}"#,
    )
    .unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker: cas\nallowBuilds:\n  '@pnpm.e2e/has-bin-and-needs-build': true\n  '@pnpm.e2e/pre-and-postinstall-scripts-example': true\n  '@pnpm.e2e/install-script-example': true\n");
    fs::write(&yaml_path, &yaml).unwrap();
    let failure = pacquet
        .with_arg("install")
        .assert()
        .failure();
    assert!(String::from_utf8_lossy(&failure.get_output().stderr).contains("casMaterialize"));
    fs::write(
        &yaml_path,
        format!("{yaml}casMaterialize:\n  - '@pnpm.e2e/has-bin-and-needs-build'\n"),
    )
    .unwrap();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "has-bin-and-needs-build"])
        .assert()
        .success();
    assert_no_node_modules(&workspace);
    drop((root, mock_instance));
}

#[test]
fn cas_remove_prunes_command_shims_and_keeps_empty_install_runnable() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/hello-world-js-bin":"1.0.0"}}"#,
    )
    .unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker: cas\n");
    fs::write(&yaml_path, yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(workspace.join(".pnpm/.bin/hello-world-js-bin").is_file());
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["remove", "@pnpm.e2e/hello-world-js-bin"])
        .assert()
        .success();
    assert!(!workspace.join(".pnpm/.bin/hello-world-js-bin").exists());
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "node", "-e", "console.log('empty install')"])
        .assert()
        .success();
    assert_no_node_modules(&workspace);
    drop((root, mock_instance));
}
