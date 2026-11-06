//! Bin and pruning scenarios run under both `nodeLinker: isolated` and
//! `nodeLinker: hoisted`.
//!
//! Each scenario observes what the two layouts share, such as the commands
//! in an importer's `.bin` and its direct dependencies, and expects the
//! same under both linkers. A fix that reaches one linker only fails here.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

const LINKERS: [&str; 2] = ["isolated", "hoisted"];

/// The commands in `<dir>/.bin`, one per command whatever shim files a
/// platform writes for it.
fn commands(dir: &Path) -> BTreeSet<String> {
    let Ok(entries) = fs::read_dir(dir.join(".bin")) else { return BTreeSet::new() };
    entries
        .map(|entry| {
            entry
                .expect("read a .bin entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .map(|name| {
            name.strip_suffix(".cmd")
                .or_else(|| name.strip_suffix(".ps1"))
                .unwrap_or(&name)
                .to_owned()
        })
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().expect("a parent dir")).expect("create the parent dir");
    fs::write(path, serde_json::to_string_pretty(value).expect("serialize JSON"))
        .expect("write JSON");
}

/// Appends to the `pnpm-workspace.yaml` the mocked registry wrote, which
/// holds its `storeDir` and `cacheDir`.
fn write_workspace_yaml(workspace: &Path, linker: &str, extra: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{yaml}nodeLinker: {linker}\n{extra}"))
        .expect("write pnpm-workspace.yaml");
}

fn install(workspace: &Path, args: &[&str]) {
    crate::_utils::pacquet_in(workspace)
        .with_arg("install")
        .with_args(args)
        .assert()
        .success();
}

/// Runs `scenario` once per linker in a fresh workspace with the mocked
/// registry, and checks that each linker observes `expected`.
fn assert_under_both_linkers<Observed: std::fmt::Debug + PartialEq>(
    expected: &Observed,
    scenario: impl Fn(&str, &Path) -> Observed,
) {
    for linker in LINKERS {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        let observed = scenario(linker, &workspace);
        assert_eq!(&observed, expected, "nodeLinker: {linker}");
        drop((root, mock_instance));
    }
}

#[test]
fn a_direct_dependency_bin_is_linked() {
    assert_under_both_linkers(&set(&["hello-world-js-bin"]), |linker, workspace| {
        write_workspace_yaml(workspace, linker, "");
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" } }),
        );
        install(workspace, &[]);
        commands(&workspace.join("node_modules"))
    });
}

#[test]
fn the_shims_of_a_removed_dependency_are_pruned() {
    assert_under_both_linkers(&(BTreeSet::new(), false), |linker, workspace| {
        write_workspace_yaml(workspace, linker, "");
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/touch-file-many-bins": "1.0.0" } }),
        );
        install(workspace, &[]);
        write_json(&workspace.join("package.json"), &json!({ "dependencies": {} }));
        install(workspace, &[]);
        let modules_dir = workspace.join("node_modules");
        (commands(&modules_dir), modules_dir.join("@pnpm.e2e/touch-file-many-bins").exists())
    });
}

#[test]
fn the_commands_a_replaced_dependency_no_longer_provides_are_pruned() {
    assert_under_both_linkers(&set(&["t"]), |linker, workspace| {
        write_workspace_yaml(workspace, linker, "");
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/touch-file-many-bins": "1.0.0" } }),
        );
        install(workspace, &[]);
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/touch-file-one-bin": "1.0.0" } }),
        );
        install(workspace, &[]);
        commands(&workspace.join("node_modules"))
    });
}

/// What a command in `<modules_dir>/.bin` runs: the file a symlinked bin
/// resolves to, or the text of a shim, which names its target.
fn command_target(modules_dir: &Path, command: &str) -> String {
    let bin = modules_dir.join(".bin").join(command);
    let is_link = fs::symlink_metadata(&bin)
        .expect("stat the bin")
        .file_type()
        .is_symlink();
    let target = if is_link {
        fs::canonicalize(&bin)
            .expect("resolve the bin")
            .to_string_lossy()
            .into_owned()
    } else {
        fs::read_to_string(&bin).expect("read the shim")
    };
    target.replace('\\', "/")
}

/// `@pnpm.e2e/hello-world-js-bin-parent` declares the command its own
/// dependency also declares.
#[test]
fn a_direct_dependency_wins_a_command_over_its_own_dependency() {
    assert_under_both_linkers(&true, |linker, workspace| {
        write_workspace_yaml(workspace, linker, "");
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" } }),
        );
        install(workspace, &[]);
        let target = command_target(&workspace.join("node_modules"), "hello-world-js-bin");
        dbg!(&target);
        target.contains("hello-world-js-bin-parent/index.js")
    });
}

#[test]
fn a_dev_dependency_bin_is_not_linked_by_a_prod_install() {
    assert_under_both_linkers(&set(&["t"]), |linker, workspace| {
        write_workspace_yaml(workspace, linker, "");
        write_json(
            &workspace.join("package.json"),
            &json!({
                "dependencies": { "@pnpm.e2e/touch-file-one-bin": "1.0.0" },
                "devDependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
            }),
        );
        install(workspace, &["--prod"]);
        commands(&workspace.join("node_modules"))
    });
}

/// The bins of `@pnpm.e2e/generated-bins` exist only once its postinstall
/// script has run.
#[test]
fn the_bins_a_build_creates_are_linked() {
    assert_under_both_linkers(&set(&["cmd1", "cmd2"]), |linker, workspace| {
        write_workspace_yaml(
            workspace,
            linker,
            "allowBuilds:\n  '@pnpm.e2e/generated-bins': true\n",
        );
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/generated-bins": "1.0.0" } }),
        );
        install(workspace, &[]);
        commands(&workspace.join("node_modules"))
    });
}

#[test]
fn a_file_the_user_put_in_the_bin_dir_is_kept() {
    assert_under_both_linkers(&set(&["my-script", "t"]), |linker, workspace| {
        write_workspace_yaml(workspace, linker, "");
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/touch-file-many-bins": "1.0.0" } }),
        );
        install(workspace, &[]);
        fs::write(workspace.join("node_modules/.bin/my-script"), "#!/bin/sh\necho mine\n")
            .expect("write a user script");
        write_json(
            &workspace.join("package.json"),
            &json!({ "dependencies": { "@pnpm.e2e/touch-file-one-bin": "1.0.0" } }),
        );
        install(workspace, &[]);
        commands(&workspace.join("node_modules"))
    });
}

/// A workspace project that depends on a sibling gets the sibling's
/// command, and loses it when the dependency is removed.
#[test]
fn a_workspace_sibling_bin_is_linked_and_pruned() {
    let expected = (set(&["sibling-tool"]), BTreeSet::new());
    assert_under_both_linkers(&expected, |linker, workspace| {
        write_workspace_yaml(workspace, linker, "packages:\n  - 'packages/*'\n");
        write_json(&workspace.join("package.json"), &json!({ "name": "root", "private": true }));
        let sibling = workspace.join("packages/sibling");
        write_json(
            &sibling.join("package.json"),
            &json!({ "name": "sibling", "version": "1.0.0", "bin": { "sibling-tool": "cli.js" } }),
        );
        fs::write(sibling.join("cli.js"), "#!/usr/bin/env node\n").expect("write cli.js");
        let app = workspace.join("packages/app");
        write_json(
            &app.join("package.json"),
            &json!({ "name": "app", "version": "1.0.0", "dependencies": { "sibling": "workspace:*" } }),
        );
        install(workspace, &[]);
        let linked = commands(&app.join("node_modules"));
        write_json(&app.join("package.json"), &json!({ "name": "app", "version": "1.0.0" }));
        install(workspace, &[]);
        (linked, commands(&app.join("node_modules")))
    });
}

/// The dependency stays, so only the stale-shim prune can remove the
/// command it stopped declaring.
#[test]
fn a_command_a_kept_dependency_stops_declaring_is_pruned() {
    assert_under_both_linkers(&set(&["renamed-tool"]), |linker, workspace| {
        write_workspace_yaml(workspace, linker, "packages:\n  - 'packages/*'\n");
        write_json(&workspace.join("package.json"), &json!({ "name": "root", "private": true }));
        let sibling = workspace.join("packages/sibling");
        let write_sibling = |command: &str| {
            write_json(
                &sibling.join("package.json"),
                &json!({ "name": "sibling", "version": "1.0.0", "bin": { command: "cli.js" } }),
            );
        };
        write_sibling("sibling-tool");
        fs::write(sibling.join("cli.js"), "#!/usr/bin/env node\n").expect("write cli.js");
        let app = workspace.join("packages/app");
        write_json(
            &app.join("package.json"),
            &json!({ "name": "app", "version": "1.0.0", "dependencies": { "sibling": "workspace:*" } }),
        );
        install(workspace, &[]);
        write_sibling("renamed-tool");
        install(workspace, &[]);
        commands(&app.join("node_modules"))
    });
}
