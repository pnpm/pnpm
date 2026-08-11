//! End-to-end coverage for `syncInjectedDepsAfterScripts`.
//!
//! An injected dependency is a tree of hardlinks into the virtual
//! store, so a build script that rewrites the source package leaves
//! every injected copy pointing at the old inodes. The setting names
//! the scripts after which those copies are refreshed.

use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

/// The injected copies of `project-1` inside the virtual store: every
/// `project-1` directory under `node_modules/.pnpm`.
fn injected_copies(workspace: &Path) -> Vec<std::path::PathBuf> {
    let virtual_store = workspace.join("node_modules/.pnpm");
    let mut copies: Vec<_> = fs::read_dir(&virtual_store)
        .expect("read the virtual store")
        .filter_map(|entry| {
            let slot = entry.expect("read a virtual-store entry").path();
            let candidate = slot.join("node_modules/project-1");
            candidate.is_dir().then_some(candidate)
        })
        .collect();
    copies.sort();
    copies
}

fn write_workspace(workspace: &Path, sync_after: &str) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - 'project-*'\ninjectWorkspacePackages: true\ndedupeInjectedDeps: false\n{sync_after}",
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    fs::create_dir_all(workspace.join("project-1/distribution")).expect("mkdir project-1");
    fs::write(
        workspace.join("project-1/package.json"),
        serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "scripts": { "build": "node build.cjs" },
        })
        .to_string(),
    )
    .expect("write project-1 package.json");
    fs::write(
        workspace.join("project-1/build.cjs"),
        "require('fs').writeFileSync(__dirname + '/distribution/generated.js', 'generated')\n",
    )
    .expect("write project-1 build script");
    fs::write(workspace.join("project-1/distribution/index.js"), "original")
        .expect("write project-1 output");

    fs::create_dir_all(workspace.join("project-2")).expect("mkdir project-2");
    fs::write(
        workspace.join("project-2/package.json"),
        serde_json::json!({
            "name": "project-2",
            "version": "1.0.0",
            "dependencies": { "project-1": "workspace:1.0.0" },
        })
        .to_string(),
    )
    .expect("write project-2 package.json");
}

#[test]
fn a_listed_script_refreshes_every_injected_copy() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace(&workspace, "syncInjectedDepsAfterScripts:\n  - build\n");

    pacquet.with_arg("install").assert().success();

    let copies = injected_copies(&workspace);
    assert!(!copies.is_empty(), "the install should have injected project-1 somewhere");

    pacquet_in(&workspace.join("project-1")).with_args(["run", "build"]).assert().success();

    for copy in &copies {
        assert_eq!(
            fs::read_to_string(copy.join("distribution/generated.js"))
                .expect("read the refreshed injected copy"),
            "generated",
            "the injected copy at {copy:?} should have gained the generated file",
        );
    }

    drop((mock_instance, root));
}

#[test]
fn an_unlisted_script_leaves_the_injected_copies_alone() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace(&workspace, "");

    pacquet.with_arg("install").assert().success();

    let copies = injected_copies(&workspace);
    assert!(!copies.is_empty(), "the install should have injected project-1 somewhere");

    pacquet_in(&workspace.join("project-1")).with_args(["run", "build"]).assert().success();

    for copy in &copies {
        assert!(
            !copy.join("distribution/generated.js").exists(),
            "the injected copy at {copy:?} should not have gained the generated file",
        );
    }

    drop((mock_instance, root));
}

/// The setting reaches a non-workspace project through `PNPM_CONFIG_*`,
/// which every schema key reads. A script that already succeeded must
/// not have the run fail behind it.
#[test]
fn a_project_outside_a_workspace_still_runs_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "solo",
            "version": "1.0.0",
            "scripts": { "build": "node --version" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_SYNC_INJECTED_DEPS_AFTER_SCRIPTS", r#"["build"]"#)
        .with_args(["run", "build"])
        .assert()
        .success();

    drop(root);
}
