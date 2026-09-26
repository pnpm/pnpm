//! Injected workspace dependencies under `sharedWorkspaceLockfile: false`.
//!
//! Regression test for issue
//! [#9828](https://github.com/pnpm/pnpm/issues/9828): a project with its own
//! lockfile kept a plain copy of an injected workspace project that has a
//! `postinstall` script, while a shared lockfile hard links the copy after
//! the script runs.

#![cfg(unix)]

use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, os::unix::fs::MetadataExt};

#[test]
fn an_injected_project_with_a_postinstall_script_is_hard_linked() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'shared'\n  - 'app'\nsharedWorkspaceLockfile: false\nstrictDepBuilds: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::create_dir_all(workspace.join("shared")).expect("mkdir shared");
    fs::write(
        workspace.join("shared/package.json"),
        serde_json::json!({
            "name": "shared",
            "version": "1.0.0",
            "scripts": {
                "postinstall": r#"node -e "require('fs').writeFileSync('built.txt', '')""#,
            },
        })
        .to_string(),
    )
    .expect("write shared package.json");
    fs::write(workspace.join("shared/index.js"), "module.exports = 1").expect("write index.js");
    fs::create_dir_all(workspace.join("app")).expect("mkdir app");
    fs::write(
        workspace.join("app/package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "shared": "workspace:*" },
            "dependenciesMeta": { "shared": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write app package.json");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    for file in ["index.js", "built.txt"] {
        let copy = workspace.join("app/node_modules/shared").join(file);
        let source = workspace.join("shared").join(file);
        assert_eq!(
            fs::metadata(&copy).expect("stat the injected copy").ino(),
            fs::metadata(&source).expect("stat the source").ino(),
            "{copy:?} should be a hard link of {source:?}",
        );
    }

    drop((mock_instance, root));
}

#[test]
fn an_injected_project_that_publishes_from_a_prepared_directory_gets_the_built_content() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'shared'\n  - 'app'\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::create_dir_all(workspace.join("shared")).expect("mkdir shared");
    fs::write(
        workspace.join("shared/package.json"),
        serde_json::json!({
            "name": "shared",
            "version": "1.0.0",
            "scripts": { "prepare": "node build.cjs" },
            "publishConfig": { "directory": "dist" },
        })
        .to_string(),
    )
    .expect("write shared package.json");
    fs::write(
        workspace.join("shared/build.cjs"),
        "const fs = require('fs')\n\
         fs.mkdirSync(__dirname + '/dist', { recursive: true })\n\
         fs.copyFileSync(__dirname + '/package.json', __dirname + '/dist/package.json')\n\
         fs.writeFileSync(__dirname + '/dist/index.js', 'built')\n",
    )
    .expect("write build.cjs");
    fs::create_dir_all(workspace.join("app")).expect("mkdir app");
    fs::write(
        workspace.join("app/package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "shared": "workspace:*" },
            "dependenciesMeta": { "shared": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write app package.json");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let copy = workspace.join("app/node_modules/shared/index.js");
    assert_eq!(
        fs::read_to_string(&copy)
            .unwrap_or_else(|error| panic!("read the build output in {copy:?}: {error}")),
        "built",
    );

    drop((mock_instance, root));
}
