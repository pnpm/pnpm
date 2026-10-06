use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn assert_no_installed_packages(root: &Path) {
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.expect("walk application");
        if entry.file_name() != "node_modules" {
            continue;
        }
        for child in fs::read_dir(entry.path()).unwrap() {
            let child = child.unwrap().path();
            let name = child
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(name.starts_with('.'), "{}", child.display());
        }
    }
}

#[test]
fn loaded_linker_cli_object_applies_exclusions_before_deriving_installation_layout() {
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
    let flag = r#"--config.node-linker={"type":"loaded","excluded":["@pnpm.e2e/pkg-with-1-dep"]}"#;
    pacquet
        .with_args(["install", flag])
        .assert()
        .success();
    assert_no_installed_packages(&workspace);
    let manifest: Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["packages"]["@pnpm.e2e/pkg-with-1-dep@100.0.0"]["resolution"], "node");
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["config", "get", "nodeLinker", "--json", flag])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        serde_json::json!({"type": "loaded", "excluded": ["@pnpm.e2e/pkg-with-1-dep"]}),
    );
    drop((root, mock_instance));
}

#[test]
fn cas_install_runs_scripts_bins_and_children_without_installed_packages() {
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
    yaml.push_str("nodeLinker:\n  type: loaded\n");
    fs::write(yaml_path, yaml).unwrap();
    fs::write(workspace.join("app.cjs"), "const assert = require('node:assert/strict'); const value = require('@pnpm.e2e/pkg-with-1-dep')(); assert.equal(value.name, '@pnpm.e2e/dep-of-pkg-with-1-dep'); require('node:child_process').execFileSync(process.execPath, ['-e', `require('@pnpm.e2e/pkg-with-1-dep')()`], {stdio: 'inherit'}); console.log('CAS works');").unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_no_installed_packages(&workspace);
    #[cfg(unix)]
    Command::new(workspace.join("node_modules/.bin/hello-world-js-bin"))
        .with_current_dir(&workspace)
        .env_remove("NODE_OPTIONS")
        .assert()
        .success();
    assert!(workspace.join("node_modules/.pnpm-workspace-state-v1.json").is_file());
    let loader_mtime = fs::metadata(workspace.join("node_modules/.pnpm/.store-loader.mjs"))
        .unwrap()
        .modified()
        .unwrap();
    for args in [
        vec!["run", "test"],
        vec!["run", "test"],
        vec!["exec", "node", "app.cjs"],
        vec!["node", "app.cjs"],
    ] {
        let output = Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_arg("--config.verify-deps-before-run=error")
            .with_args(args)
            .env_remove("NODE_OPTIONS")
            .assert()
            .success();
        assert!(String::from_utf8_lossy(&output.get_output().stdout).contains("CAS works"));
    }
    assert_eq!(
        fs::metadata(workspace.join("node_modules/.pnpm/.store-loader.mjs"))
            .unwrap()
            .modified()
            .unwrap(),
        loader_mtime,
    );
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
    assert_no_installed_packages(&workspace);
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["run", "test"])
        .assert()
        .success();
    let manifest: Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    assert!(manifest["packages"]["@pnpm.e2e/pkg-with-1-dep@100.0.0"]["files"].is_object());
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["install", "--prod", "--frozen-lockfile"])
        .assert()
        .success();
    let production: Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    assert!(production["packages"]["."]["dependencies"]["@pnpm.e2e/hello-world-js-bin"].is_null());
    assert!(!workspace.join("node_modules/.bin/hello-world-js-bin").exists());
    assert_no_installed_packages(&workspace);
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
    yaml.push_str("nodeLinker:\n  type: loaded\n  excluded:\n    - '@pnpm.e2e/pkg-with-1-dep'\n");
    fs::write(yaml_path, yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_no_installed_packages(&workspace);
    let manifest: Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
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
    fs::write(workspace.join("packages/child/package.json"), r#"{"name":"child","version":"1.0.0","main":"index.cjs","bin":{"nested":"bin.cjs"},"scripts":{"test":"node index.cjs"},"dependencies":{"aliased":"npm:@pnpm.e2e/pkg-with-1-dep@100.0.0"}}"#).unwrap();
    fs::write(workspace.join("packages/child/index.cjs"), "require('node:assert/strict').equal(require('aliased')().name, '@pnpm.e2e/dep-of-pkg-with-1-dep'); module.exports = 42;").unwrap();
    fs::write(workspace.join("packages/child/bin.cjs"), r"const assert = require('node:assert/strict'); const options = process.env.NODE_OPTIONS; assert.equal(options.split(/\s+/).filter(option => option.includes('.store-loader.mjs')).length, 1); if (!process.env.NESTED_SHIM) require('node:child_process').execFileSync('nested', [], {env: {...process.env, NESTED_SHIM: 'true'}, stdio: 'inherit', shell: process.platform === 'win32'});").unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker:\n  type: loaded\npackages:\n  - packages/*\n");
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
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "exec", "nested"])
        .env_remove("NODE_OPTIONS")
        .assert()
        .success();
    assert_no_installed_packages(&workspace);
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
    yaml.push_str("nodeLinker:\n  type: loaded\n");
    fs::write(&yaml_path, &yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    for materialize in [true, false] {
        let config = if materialize {
            format!("{yaml}  excluded:\n    - '@pnpm.e2e/pkg-with-1-dep'\n")
        } else {
            yaml.clone()
        };
        fs::write(&yaml_path, config).unwrap();
        fs::remove_file(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap();
        Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();
        let manifest: Value = serde_json::from_slice(
            &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
        )
        .unwrap();
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
    for filename in [".store-manifest.json", ".store-loader.mjs"] {
        fs::remove_file(workspace.join("node_modules/.pnpm").join(filename)).unwrap();
        Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_args(["exec", "node", "-e", "require('@pnpm.e2e/pkg-with-1-dep')()"])
            .env_remove("NODE_OPTIONS")
            .assert()
            .success();
        assert!(
            workspace
                .join("node_modules/.pnpm")
                .join(filename)
                .is_file(),
        );
    }
    assert_no_installed_packages(&workspace);
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
    yaml.push_str("allowBuilds:\n  '@pnpm.e2e/has-bin-and-needs-build': true\n  '@pnpm.e2e/pre-and-postinstall-scripts-example': true\n  '@pnpm.e2e/install-script-example': true\nnodeLinker:\n  type: loaded\n");
    fs::write(&yaml_path, &yaml).unwrap();
    let failure = pacquet
        .with_arg("install")
        .assert()
        .failure();
    assert!(String::from_utf8_lossy(&failure.get_output().stderr).contains("nodeLinker.excluded"));
    fs::write(
        &yaml_path,
        format!("{yaml}  excluded:\n    - '@pnpm.e2e/has-bin-and-needs-build'\n"),
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
    assert_no_installed_packages(&workspace);
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
    yaml.push_str("nodeLinker:\n  type: loaded\n");
    fs::write(&yaml_path, yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(workspace.join("node_modules/.bin/hello-world-js-bin").is_file());
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["remove", "@pnpm.e2e/hello-world-js-bin"])
        .assert()
        .success();
    assert!(!workspace.join("node_modules/.bin/hello-world-js-bin").exists());
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "node", "-e", "console.log('empty install')"])
        .assert()
        .success();
    assert_no_installed_packages(&workspace);
    drop((root, mock_instance));
}

#[test]
fn loaded_project_local_store_survives_prune_and_requires_registration() {
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
    let mut yaml = fs::read_to_string(&yaml_path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("storeDir:"))
        .collect::<Vec<_>>()
        .join("\n");
    yaml.push('\n');
    yaml.push_str("nodeLinker:\n  type: loaded\nstoreDir: store\n");
    fs::write(&yaml_path, yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
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
    assert_no_installed_packages(&workspace);
    let manifest: Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    let registry = Path::new(manifest["storeDir"].as_str().unwrap()).join("projects");
    fs::remove_dir_all(&registry).unwrap();
    fs::write(&registry, "blocked registry").unwrap();
    let failure = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .failure();
    assert!(
        String::from_utf8_lossy(&failure.get_output().stderr)
            .contains("ERR_PNPM_STORE_DIR_REGISTER_PROJECT_CREATE_REGISTRY_DIR"),
    );
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("frozenStore: true\n");
    fs::write(&yaml_path, yaml).unwrap();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&registry).unwrap(), "blocked registry");
    drop((root, mock_instance));
}

#[test]
fn cas_loader_starts_when_a_package_ships_node_modules_files() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let mut tarball = tar::Builder::new(Vec::new());
    for (path, body) in [
        ("package/package.json", r#"{"name":"fixtures-in-node-modules","version":"1.0.0"}"#),
        ("package/index.js", "module.exports = 'loaded'"),
        ("package/test/node_modules/fixture.js", "module.exports = 'fixture'"),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        tarball.append_data(&mut header, path, body.as_bytes()).unwrap();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &tarball.into_inner().unwrap()).unwrap();
    fs::write(workspace.join("dep.tgz"), encoder.finish().unwrap()).unwrap();
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"fixtures-in-node-modules":"file:dep.tgz"}}"#,
    )
    .unwrap();
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap();
    yaml.push_str("nodeLinker:\n  type: loaded\n");
    fs::write(yaml_path, yaml).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    let manifest: Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    let files = manifest["packages"]
        .as_object()
        .unwrap()
        .values()
        .find_map(|package| {
            package["files"]
                .as_object()
                .filter(|files| files.contains_key("index.js"))
        })
        .expect("the tarball dependency loads from CAS");
    assert!(files.contains_key("test/node_modules/fixture.js"), "{files:?}");
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "node", "-e", "console.log(require('fixtures-in-node-modules'))"])
        .env_remove("NODE_OPTIONS")
        .assert()
        .success();
    assert_eq!(String::from_utf8_lossy(&output.get_output().stdout).trim(), "loaded");
    drop((root, mock_instance));
}
