//! `sideEffectsCacheExclude`: builds that must run in every project
//! because their output depends on the environment.
//!
//! Regression coverage for <https://github.com/pnpm/pnpm/issues/5271>.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, path::Path, process::Command};

/// Writes the environment of its `install` script to `env.json`.
const ENV_WRITER: &str = "@pnpm.e2e/write-lifecycle-env";

#[test]
fn a_cached_build_reaches_a_project_that_builds_in_another_environment() {
    assert_eq!(build_flavors(&Layout::default()), ["first", "first"]);
}

#[test]
fn an_excluded_package_is_built_in_every_project() {
    let layout = Layout { exclude: true, ..Layout::default() };
    assert_eq!(build_flavors(&layout), ["first", "second"]);
}

/// The first project's flavor is checked after the second install too: a
/// second build into a slot the two projects shared would overwrite it.
#[test]
fn an_excluded_package_is_built_in_every_project_under_the_global_virtual_store() {
    let layout = Layout { exclude: true, global_virtual_store: true };
    assert_eq!(build_flavors(&layout), ["first", "second"]);
}

#[derive(Default)]
struct Layout {
    exclude: bool,
    global_virtual_store: bool,
}

/// Install [`ENV_WRITER`] in two projects that share one store, with
/// `BUILD_FLAVOR` set to `first` and then `second`, and return the
/// flavor each project's build output records once both are installed.
fn build_flavors(layout: &Layout) -> [String; 2] {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    configure(&workspace, layout);
    let second = root.path().join("second");
    fs::create_dir(&second).expect("create the second project");
    for file in [".npmrc", "pnpm-workspace.yaml", "package.json"] {
        fs::copy(workspace.join(file), second.join(file)).expect("copy the project files");
    }

    install(&workspace, "first");
    assert_eq!(build_flavor(&workspace), "first");
    install(&second, "second");
    let flavors = [build_flavor(&workspace), build_flavor(&second)];

    drop((root, mock_instance));
    flavors
}

fn configure(workspace: &Path, layout: &Layout) {
    let mut settings = format!("allowBuilds:\n  '{ENV_WRITER}': true\n");
    if layout.exclude {
        settings.push_str(&format!("sideEffectsCacheExclude:\n  - '{ENV_WRITER}'\n"));
    }
    if layout.global_virtual_store {
        crate::_utils::enable_gvs_in_workspace_yaml(workspace, &settings);
    } else {
        let yaml_path = workspace.join("pnpm-workspace.yaml");
        let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
        yaml.push_str(&settings);
        fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    }
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { ENV_WRITER: "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
}

fn install(project: &Path, flavor: &str) {
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .without_ambient_pnpm_config()
        .with_current_dir(project)
        .with_env("BUILD_FLAVOR", flavor)
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    assert!(
        output.status.success(),
        "install failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn build_flavor(project: &Path) -> String {
    let env_json = project
        .join("node_modules")
        .join(ENV_WRITER)
        .join("env.json");
    let env: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&env_json).expect("read env.json"))
            .expect("parse env.json");
    env["BUILD_FLAVOR"]
        .as_str()
        .expect("the build recorded BUILD_FLAVOR")
        .to_string()
}
