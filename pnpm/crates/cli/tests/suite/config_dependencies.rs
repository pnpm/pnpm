use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_config::WorkspaceSettings;
use pnpm_lockfile::EnvLockfile;
use pnpm_modules_yaml::{
    Host,
    NodeLinker,
    read_modules_manifest,
};
use pnpm_testing_utils::{
    bin::{
        AddMockedRegistry,
        CommandTempCwd,
    },
    fs::{
        bump_mtime,
        is_symlink_or_junction,
    },
};
use pnpm_workspace_state::ConfigDependency;
use std::{
    fs,
    path::Path,
    process::{
        Command,
        Stdio,
    },
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// `pacquet install` resolves the `configDependencies` declared in
/// `pnpm-workspace.yaml`, links them under `node_modules/.pnpm-config`,
/// and records them in the env lockfile (the first document of
/// `pnpm-lock.yaml`).
#[test]
fn installs_configurational_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");

    // Append a configDependencies block to the workspace manifest the
    // mocked-registry helper already wrote.
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("\nconfigDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let installed = workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json");
    assert!(installed.exists(), "config dep must be linked under .pnpm-config");

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.starts_with("---\n"), "env document must lead pnpm-lock.yaml");
    assert!(lockfile.contains("configDependencies:"));
    assert!(lockfile.contains("@pnpm.e2e/foo"));

    drop((root, mock_instance));
}

#[test]
fn config_dependency_install_waits_for_the_store_operation_lock() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir.clone());
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("\nconfigDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    let prune_lock = store_dir.lock_for_prune().expect("lock store for prune");
    let output_path = workspace.join("config-dependency-lock.ndjson");
    let mut install = pacquet_at(&workspace)
        .with_args(["--reporter=ndjson", "--loglevel=debug", "install"])
        .stdout(Stdio::null())
        .stderr(fs::File::create(&output_path).expect("create install output"))
        .spawn()
        .expect("spawn install");
    _utils::wait_for_child_output(
        &mut install,
        &output_path,
        "Waiting for the configuration dependency store operation lock",
    );
    _utils::assert_child_output_stays_absent(
        &mut install,
        &output_path,
        "Acquired the configuration dependency store operation lock",
    );

    drop(prune_lock);
    assert!(_utils::wait_for_child(&mut install).success());
    let output = fs::read_to_string(&output_path).expect("read completed install output");
    assert!(
        output.contains("Acquired the configuration dependency store operation lock"),
        "{output}",
    );
    assert!(
        workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists(),
        "config dependency must be materialized after the prune lock is released",
    );

    drop((root, mock_instance));
}

/// A second `pacquet install` with the env lockfile already in place is
/// a no-op for config deps — it must still succeed and keep the link.
#[test]
fn second_install_keeps_config_dependency() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("\nconfigDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists(),
        "config dep must remain linked after a repeat install",
    );

    drop((root, mock_instance));
}

/// An `updateConfig` pnpmfile hook mutates the resolved config before
/// the install runs: a hook that flips `nodeLinker` to `hoisted` changes
/// the on-disk layout (the dependency becomes a real directory rather
/// than a symlink into the virtual store).
#[test]
fn update_config_hook_mutates_config_before_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) {\n  config.nodeLinker = 'hoisted';\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let dep = workspace.join("node_modules/@pnpm.e2e/foo");
    assert!(dep.join("package.json").exists(), "dependency is installed");
    // Hoisted linking materializes the dep as a real directory, where
    // isolated would link it into the virtual store, so this is what
    // proves the hook flipped `nodeLinker`.
    assert!(
        !is_symlink_or_junction(&dep).unwrap(),
        "updateConfig forced nodeLinker: hoisted, so the dep is a real directory, not a symlink",
    );

    drop((root, mock_instance));
}

/// `pacquet add --config <pkg>@<version>` resolves and installs the
/// package as a configurational dependency, writing the clean specifier
/// to `pnpm-workspace.yaml` and linking it under `.pnpm-config`.
#[test]
fn add_config_writes_workspace_yaml_and_installs() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");

    pacquet_at(&workspace)
        .with_arg("add")
        .with_arg("--config")
        .with_arg("@pnpm.e2e/foo@100.0.0")
        .assert()
        .success();

    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    eprintln!("pnpm-workspace.yaml:\n{yaml}");
    assert!(yaml.contains("configDependencies:"), "configDependencies block written");
    assert!(yaml.contains("@pnpm.e2e/foo"));
    assert!(yaml.contains("100.0.0"));
    // The pre-existing storeDir setting must survive the format-preserving edit.
    assert!(yaml.contains("storeDir:"), "untouched settings are preserved");

    assert!(
        workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists(),
        "config dep linked into .pnpm-config",
    );

    drop((root, mock_instance));
}

#[test]
fn add_config_accepts_multiple_package_selectors_in_one_operation() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");

    pacquet_at(&workspace)
        .with_args(["add", "--config", "@pnpm.e2e/foo@100.0.0", "@pnpm.e2e/bar@100.0.0"])
        .assert()
        .success();

    let (_, settings) = WorkspaceSettings::find_and_load(&workspace)
        .expect("read pnpm-workspace.yaml")
        .expect("workspace manifest exists");
    let config_dependencies = settings.config_dependencies.expect("configDependencies map");
    assert_eq!(config_dependencies.len(), 2);
    for package_name in ["@pnpm.e2e/foo", "@pnpm.e2e/bar"] {
        assert_eq!(
            config_dependencies.get(package_name),
            Some(&ConfigDependency::VersionWithIntegrity("100.0.0".to_string())),
        );
    }

    let env_lockfile =
        EnvLockfile::read(&workspace).expect("read env lockfile").expect("env lockfile exists");
    let root_importer = &env_lockfile.importers[EnvLockfile::ROOT_IMPORTER_KEY];
    for package_name in ["@pnpm.e2e/foo", "@pnpm.e2e/bar"] {
        let dependency = &root_importer.config_dependencies[package_name];
        assert_eq!(dependency.specifier, "100.0.0");
        assert_eq!(dependency.version, "100.0.0");

        let installed = workspace
            .join("node_modules/.pnpm-config")
            .join(package_name)
            .join("package.json");
        assert!(installed.exists(), "config dependency installed at {}", installed.display());
    }

    drop((root, mock_instance));
}

#[test]
fn add_config_validates_all_selectors_before_writing_files() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    for path in [
        workspace.join("package.json"),
        workspace.join("pnpm-lock.yaml"),
        workspace.join("pnpm-workspace.yaml"),
        workspace.join("node_modules"),
    ] {
        assert!(!path.exists(), "precondition: {} does not exist", path.display());
    }

    pacquet
        .with_args(["add", "--config", "valid-package@1.0.0", "file:../missing"])
        .assert()
        .failure();

    for path in [
        workspace.join("package.json"),
        workspace.join("pnpm-lock.yaml"),
        workspace.join("pnpm-workspace.yaml"),
        workspace.join("node_modules"),
    ] {
        assert!(!path.exists(), "invalid selector must not create {}", path.display());
    }

    drop(root);
}

/// An `updateConfig` hook can inject a `catalogs` entry that the install
/// then resolves a `catalog:` specifier against — even though
/// `pnpm-workspace.yaml` declares no catalog. Without the hook the
/// `catalog:` dependency would have no entry to resolve to.
#[test]
fn update_config_hook_injects_catalog() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "catalog:" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) {\n  config.catalogs = { default: { '@pnpm.e2e/foo': '100.0.0' } };\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+foo@100.0.0").exists(),
        "the catalog: dep resolved to the version the updateConfig hook injected",
    );

    drop((root, mock_instance));
}

#[test]
fn update_config_observes_and_can_replace_the_cli_store_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    _utils::enable_gvs_in_workspace_yaml(&workspace, "");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "const fs = require('fs');\nconst path = require('path');\nmodule.exports = { hooks: { updateConfig (config) {\n  fs.writeFileSync(path.join(__dirname, 'observed-store.txt'), String(config.storeDir));\n  config.storeDir = 'hook-store';\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    pacquet_at(&workspace)
        .with_args(["install", "--store-dir=cli-store"])
        .assert()
        .success();

    let observed = fs::read_to_string(workspace.join("observed-store.txt"))
        .expect("read store observed by updateConfig");
    assert_eq!(observed, "cli-store");

    let modules = pnpm_modules_yaml::read_modules_layout::<pnpm_modules_yaml::Host>(
        &workspace.join("node_modules"),
    )
    .expect("read .modules.yaml")
    .expect(".modules.yaml exists");
    let hook_store =
        dunce::canonicalize(&workspace).expect("canonicalize workspace").join("hook-store/v11");
    assert_eq!(
        dunce::canonicalize(&modules.store_dir).expect("canonicalize recorded store"),
        hook_store,
    );
    assert_eq!(
        dunce::canonicalize(&modules.virtual_store_dir)
            .expect("canonicalize recorded virtual store"),
        hook_store.join("links"),
    );
    let index_path = hook_store.join("index.db");
    eprintln!("Checking for hook store index: {}", index_path.display());
    assert!(index_path.is_file());

    drop((root, mock_instance));
}

#[test]
fn update_config_observes_an_empty_cli_store_dir() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: yaml-store\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "const fs = require('fs');\nconst path = require('path');\nmodule.exports = { hooks: { updateConfig (config) {\n  fs.writeFileSync(path.join(__dirname, 'observed-store.txt'), JSON.stringify(config.storeDir));\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    pacquet_at(&workspace)
        .with_args(["install", "--store-dir="])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(workspace.join("observed-store.txt"))
            .expect("read store observed by updateConfig"),
        r#""""#,
    );

    drop(root);
}

/// `--ignore-pnpmfile` empties the whole pnpmfile set the config layer
/// resolves, not just the workspace `.pnpmfile.cjs`: a config
/// dependency's plugin pnpmfile stops contributing its `updateConfig`
/// hook too. `@pnpm/plugin-pnpmfile` flips the node linker to
/// `hoisted`, and the modules manifest records the linker the install
/// ran with.
#[test]
fn ignore_pnpmfile_skips_a_config_dependency_plugin_pnpmfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }).to_string(),
    )
    .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("\nconfigDependencies:\n  '@pnpm/plugin-pnpmfile': 1.0.0\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    let recorded_node_linker = || {
        read_modules_manifest::<Host>(&workspace.join("node_modules"))
            .expect("read the modules manifest")
            .expect("the install wrote a modules manifest")
            .node_linker
    };

    pacquet_at(&workspace)
        .with_args(["install", "--ignore-pnpmfile"])
        .assert()
        .success();
    assert_eq!(
        recorded_node_linker(),
        Some(NodeLinker::Isolated),
        "the plugin's updateConfig hook must not reach the install",
    );

    // Install again with the plugin honored, so the assertion above
    // cannot pass on a fixture whose hook never ran.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        recorded_node_linker(),
        Some(NodeLinker::Hoisted),
        "without the flag the plugin's updateConfig hook sets the linker",
    );

    drop((root, mock_instance));
}

/// Two branches that each added a config dependency conflict inside the
/// env document — the *first* YAML document of `pnpm-lock.yaml` — where
/// the main lockfile's own conflict recovery never looks.
#[test]
fn install_merges_a_conflicted_env_document() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");

    let ours = env_document_locking(&workspace, "'@pnpm.e2e/foo': 100.0.0");
    let theirs = env_document_locking(&workspace, "'@pnpm.e2e/bar': 100.0.0");
    assert_ne!(ours, theirs, "the conflict sides must lock different config dependencies");

    set_config_dependencies(&workspace, "'@pnpm.e2e/foo': 100.0.0\n  '@pnpm.e2e/bar': 100.0.0");
    let main_document = fs::read_to_string(workspace.join("pnpm-lock.yaml"))
        .expect("read lockfile")
        .rsplit_once("\n---\n")
        .expect("combined lockfile")
        .1
        .to_string();
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        format!("---\n<<<<<<< HEAD\n{ours}=======\n{theirs}>>>>>>> branch\n---\n{main_document}"),
    )
    .expect("write conflicted lockfile");
    bump_mtime(&workspace.join("pnpm-lock.yaml"));

    let install = pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&install.get_output().stdout);
    eprintln!("STDOUT:\n{stdout}");
    assert!(stdout.contains("Merge conflict detected in pnpm-lock.yaml and successfully merged"));

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(!lockfile.contains("<<<<<<<"), "the markers must be gone:\n{lockfile}");
    let env =
        EnvLockfile::read(&workspace).expect("the env document parses").expect("env document");
    let config_deps = &env.importers[EnvLockfile::ROOT_IMPORTER_KEY].config_dependencies;
    dbg!(config_deps);
    assert!(config_deps.contains_key("@pnpm.e2e/foo"), "our side's config dep survives");
    assert!(config_deps.contains_key("@pnpm.e2e/bar"), "their side's config dep survives");

    drop((root, mock_instance));
}

/// Install `config_deps` and return the env document the install wrote,
/// as the text one side of a conflict would carry.
fn env_document_locking(workspace: &Path, config_deps: &str) -> String {
    set_config_dependencies(workspace, config_deps);
    let _ = fs::remove_file(workspace.join("pnpm-lock.yaml"));
    pacquet_at(workspace)
        .with_arg("install")
        .assert()
        .success();
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    let (env, _) = lockfile
        .strip_prefix("---\n")
        .expect("env document leads the lockfile")
        .split_once("\n---\n")
        .expect("combined lockfile");
    format!("{env}\n")
}

fn set_config_dependencies(workspace: &Path, config_deps: &str) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let base: String = yaml
        .lines()
        .take_while(|line| !line.starts_with("configDependencies:"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&yaml_path, format!("{base}\nconfigDependencies:\n  {config_deps}\n"))
        .expect("write pnpm-workspace.yaml");
}
