//! A frozen offline reinstall into a `modulesDir` outside the project puts
//! the root project's package links and command shims there, as the first
//! install did. Covers the configurations reported in
//! <https://github.com/pnpm/pnpm/issues/17889>.

use super::{write_manifest, yaml_path};
use crate::_utils::{append_workspace_yaml_key, pacquet_in};
use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

#[test]
fn frozen_offline_install_keeps_links_in_an_external_modules_dir() {
    for source in [ModulesDirSource::WorkspaceYaml, ModulesDirSource::CliFlag] {
        for virtual_store in [VirtualStore::Default, VirtualStore::Explicit] {
            check_frozen_offline_modules_dir(source, virtual_store);
        }
    }
}

/// Where the install reads `modulesDir` from.
#[derive(Debug, Clone, Copy)]
enum ModulesDirSource {
    WorkspaceYaml,
    CliFlag,
}

/// Whether `pnpm-workspace.yaml` names `virtualStoreDir` explicitly or
/// leaves it at its `<modulesDir>/.pnpm` default.
#[derive(Debug, Clone, Copy)]
enum VirtualStore {
    Default,
    Explicit,
}

#[derive(Debug, Clone, Copy)]
enum InstallMode {
    Initial,
    FrozenOffline,
}

struct Scenario<'a> {
    workspace: &'a Path,
    modules: &'a Path,
    source: ModulesDirSource,
    virtual_store: VirtualStore,
}

fn check_frozen_offline_modules_dir(source: ModulesDirSource, virtual_store: VirtualStore) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let modules = workspace
        .parent()
        .expect("workspace parent")
        .join("external/node_modules");
    match source {
        ModulesDirSource::WorkspaceYaml => {
            append_workspace_yaml_key(&workspace, "modulesDir", yaml_path(&modules));
        }
        ModulesDirSource::CliFlag => {}
    }
    match virtual_store {
        VirtualStore::Default => {}
        VirtualStore::Explicit => {
            append_workspace_yaml_key(
                &workspace,
                "virtualStoreDir",
                yaml_path(&modules.join(".pnpm")),
            );
        }
    }
    append_workspace_yaml_key(&workspace, "preferSymlinkedExecutables", "true");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        }),
    );
    let scenario = Scenario { workspace: &workspace, modules: &modules, source, virtual_store };
    install_with_external_modules(&scenario, InstallMode::Initial);
    fs::remove_dir_all(&modules).expect("remove modules directory");
    let default_modules = workspace.join("node_modules");
    if default_modules.exists() {
        fs::remove_dir_all(default_modules).expect("remove default modules directory");
    }
    install_with_external_modules(&scenario, InstallMode::FrozenOffline);
    drop((root, mock_instance));
}

fn install_with_external_modules(scenario: &Scenario<'_>, mode: InstallMode) {
    let Scenario {
        workspace,
        modules,
        source,
        virtual_store,
    } = *scenario;
    let mut command = pacquet_in(workspace);
    command.arg("install");
    match source {
        ModulesDirSource::WorkspaceYaml => {}
        ModulesDirSource::CliFlag => {
            command.arg("--modules-dir").arg(modules);
        }
    }
    match mode {
        InstallMode::Initial => {}
        InstallMode::FrozenOffline => {
            command.args(["--offline", "--frozen-lockfile"]);
        }
    }
    command.assert().success();
    let context = format!("{source:?} {virtual_store:?} {mode:?}, modules: {}", modules.display());
    assert!(modules.join("@pnpm.e2e/hello-world-js-bin/package.json").exists(), "{context}");
    let bin_name = if cfg!(windows) { "hello-world-js-bin.cmd" } else { "hello-world-js-bin" };
    let bin = modules.join(".bin").join(bin_name);
    let output = Command::new(&bin)
        .output()
        .unwrap_or_else(|error| panic!("{context}: {}: {error}", bin.display()));
    assert!(
        output.status.success(),
        "{context}: {}: {}",
        bin.display(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(), "{context}");
}
