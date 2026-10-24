use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[test]
fn repeat_install_relinks_changed_workspace_bins() {
    for linker in ["isolated", "hoisted"] {
        for optimistic in ["true", "false"] {
            assert_relinked_bins(linker, optimistic);
        }
    }
}

fn assert_relinked_bins(linker: &str, optimistic: &str) {
    let fixture = workspace(linker, optimistic);
    let root = &fixture.workspace;
    for (bin, expected) in [
        (Value::Null, vec![]),
        (json!({"a-one": "one.js"}), vec![("a-one", "one")]),
        (json!({"a-one": "one.js", "a-two": "two.js"}), vec![("a-one", "one"), ("a-two", "two")]),
        (
            json!({"a-one": "two.js", "a-renamed": "one.js"}),
            vec![("a-one", "two"), ("a-renamed", "one")],
        ),
        (json!("two.js"), vec![("a", "two")]),
        (Value::Null, vec![]),
    ] {
        write_bins(root, &bin);
        let lockfile = fs::read(root.join("pnpm-lock.yaml")).ok();
        let mut install = pnpm(root);
        install.args(["install", "--offline", "--ignore-scripts"]);
        if lockfile.is_some() {
            install.arg("--frozen-lockfile");
        }
        install.assert().success();
        if let Some(lockfile) = lockfile {
            assert_eq!(fs::read(root.join("pnpm-lock.yaml")).unwrap(), lockfile);
        }
        for (name, output) in expected {
            assert_bin(root, "root", name, output);
        }
    }
    pnpm(root)
        .args(["install", "--offline", "--ignore-scripts"])
        .assert()
        .success();
}

#[test]
fn filtered_installs_keep_unselected_workspace_bins_stale() {
    for linker in ["isolated", "hoisted"] {
        let fixture = workspace(linker, "true");
        let root = &fixture.workspace;
        write_bins(root, &json!({"a-one": "one.js"}));
        pnpm(root)
            .args(["install", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        write_bins(root, &json!({"a-one": "one.js", "a-two": "two.js"}));
        for consumer in ["b", "c", "root"] {
            pnpm(root)
                .args([
                    "--filter",
                    consumer,
                    "install",
                    "--frozen-lockfile",
                    "--offline",
                    "--ignore-scripts",
                ])
                .assert()
                .success();
            assert_bin(root, consumer, "a-two", "two");
        }
        pnpm(root)
            .args(["install", "--frozen-lockfile", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        let state_path = root.join("node_modules/.pnpm-workspace-state-v1.json");
        let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        state["validation"]
            .as_object_mut()
            .unwrap()
            .remove("workspaceBins");
        fs::write(&state_path, state.to_string()).unwrap();
        fs::remove_dir_all(root.join("node_modules/.bin")).unwrap();
        pnpm(root)
            .args(["install", "--frozen-lockfile", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        assert_bin(root, "root", "a-two", "two");
    }
}

fn workspace(linker: &str, optimistic: &str) -> CommandTempCwd<()> {
    let fixture = CommandTempCwd::init();
    let root = &fixture.workspace;
    fs::write(
        root.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\nnodeLinker: {linker}\noptimisticRepeatInstall: {optimistic}\nverifyDepsBeforeRun: false\n"),
    ).unwrap();
    for (name, dir) in
        [("root", root.clone()), ("b", root.join("packages/b")), ("c", root.join("packages/c"))]
    {
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            json!({"name": name, "private": true, "dependencies": {"a": "workspace:*"}}).to_string(
            ),
        )
        .unwrap();
    }
    let package = root.join("packages/a");
    fs::create_dir_all(&package).unwrap();
    for (file, output) in [("one.js", "one"), ("two.js", "two")] {
        fs::write(package.join(file), format!("#!/usr/bin/env node\nconsole.log('{output}')\n"))
            .unwrap();
    }
    fixture
}

fn write_bins(root: &Path, bin: &Value) {
    fs::write(
        root.join("packages/a/package.json"),
        json!({"name": "a", "version": "1.0.0", "bin": bin}).to_string(),
    )
    .unwrap();
}

fn pnpm(root: &Path) -> Command {
    Command::cargo_bin("pnpm").unwrap().with_current_dir(root)
}

fn assert_bin(root: &Path, consumer: &str, name: &str, output: &str) {
    pnpm(root)
        .args(["--filter", consumer, "--config.verify-deps-before-run=error", "exec", name])
        .assert()
        .success()
        .stdout(format!("{output}\n"));
}
