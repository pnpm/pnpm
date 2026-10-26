use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, io, path::Path, process::Command};

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
        let expected_names: Vec<&str> = expected
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_workspace_installed_bins(root, linker, &expected_names);
        for (name, output) in expected {
            assert_bin(root, "root", name, output);
        }
    }
    write_bins(root, &json!({"a-one": "one.js", "a-two": "two.js"}));
    pnpm(root)
        .args(["install", "--offline", "--ignore-scripts"])
        .assert()
        .success();
    assert_installed_bins(&root.join("node_modules/.bin"), &["a-one", "a-two"]);

    write_bins(root, &json!({"a-one": "one.js"}));
    pnpm(root)
        .args(["install", "--offline", "--ignore-scripts"])
        .assert()
        .success();
    assert_installed_bins(&root.join("node_modules/.bin"), &["a-one"]);

    write_bins(root, &Value::Null);
    pnpm(root)
        .args(["install", "--force", "--offline", "--ignore-scripts"])
        .assert()
        .success();
    assert_installed_bins(&root.join("node_modules/.bin"), &[]);
}

#[test]
fn hoisted_workspace_package_prunes_removed_bins_from_root() {
    for linker in ["isolated", "hoisted"] {
        let fixture = CommandTempCwd::init();
        let root = &fixture.workspace;
        fs::write(
            root.join("pnpm-workspace.yaml"),
            format!(
                "packages:\n  - packages/*\nnodeLinker: {linker}\nhoistWorkspacePackages: true\npublicHoistPattern:\n  - '*'\nverifyDepsBeforeRun: false\n",
            ),
        )
        .unwrap();
        fs::write(root.join("package.json"), json!({"name": "root", "private": true}).to_string())
            .unwrap();
        let pkg_b = root.join("packages/b");
        fs::create_dir_all(&pkg_b).unwrap();
        fs::write(
            pkg_b.join("package.json"),
            json!({"name": "b", "private": true, "dependencies": {"a": "workspace:*"}}).to_string(),
        )
        .unwrap();
        let pkg_a = root.join("packages/a");
        fs::create_dir_all(&pkg_a).unwrap();
        for (file, output) in [("one.js", "one"), ("two.js", "two")] {
            fs::write(pkg_a.join(file), format!("#!/usr/bin/env node\nconsole.log('{output}')\n"))
                .unwrap();
        }

        write_bins(root, &json!({"a-one": "one.js", "a-two": "two.js"}));
        pnpm(root)
            .args(["install", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        assert_installed_bins(&root.join("node_modules/.bin"), &["a-one", "a-two"]);

        write_bins(root, &json!({"a-renamed": "two.js"}));
        pnpm(root)
            .args(["install", "--frozen-lockfile", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        assert_installed_bins(&root.join("node_modules/.bin"), &["a-renamed"]);

        write_bins(root, &Value::Null);
        pnpm(root)
            .args(["install", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        assert_installed_bins(&root.join("node_modules/.bin"), &[]);
    }
}

#[test]
fn symlinked_workspace_project_retains_and_prunes_bins_under_both_linkers() {
    for linker in ["isolated", "hoisted"] {
        let fixture = workspace(linker, "false");
        let root = &fixture.workspace;
        let external_dir = fixture.root
            .path()
            .join(format!("external-{linker}"));
        fs::create_dir_all(&external_dir).unwrap();
        let external_b = external_dir.join("b");
        fs::rename(root.join("packages/b"), &external_b).unwrap();
        pnpm_fs::symlink_dir(&root.join("packages/a"), &external_dir.join("a")).unwrap();
        pnpm_fs::symlink_dir(&external_b, &root.join("packages/b")).unwrap();

        write_bins(root, &json!({"a-one": "one.js", "a-two": "two.js"}));
        pnpm(root)
            .args(["install", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        assert_installed_bins(&external_b.join("node_modules/.bin"), &["a-one", "a-two"]);

        write_bins(root, &json!({"a-renamed": "two.js"}));
        pnpm(root)
            .args(["install", "--frozen-lockfile", "--offline", "--ignore-scripts"])
            .assert()
            .success();
        assert_installed_bins(&external_b.join("node_modules/.bin"), &["a-renamed"]);
    }
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

fn assert_workspace_installed_bins(root: &Path, linker: &str, expected: &[&str]) {
    assert_installed_bins(&root.join("node_modules/.bin"), expected);
    if linker == "isolated" {
        for pkg in ["packages/b", "packages/c"] {
            assert_installed_bins(&root.join(pkg).join("node_modules/.bin"), expected);
        }
    }
}

fn assert_installed_bins(bins_dir: &Path, expected: &[&str]) {
    let entries = match fs::read_dir(bins_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            assert!(expected.is_empty(), "expected {expected:?} in missing {bins_dir:?}");
            return;
        }
        Err(error) => panic!("failed to read {bins_dir:?}: {error}"),
    };
    let actual: BTreeSet<String> = entries
        .map(|entry| {
            normalize_shim_name(
                &entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy(),
            )
        })
        .collect();
    let expected: BTreeSet<String> = expected
        .iter()
        .map(|name| normalize_shim_name(name))
        .collect();
    assert_eq!(actual, expected, "unexpected shims in {bins_dir:?}");
}

fn normalize_shim_name(name: &str) -> String {
    if !cfg!(windows) {
        return name.to_owned();
    }
    let lower = name.to_lowercase();
    [".cmd", ".ps1", ".exe"]
        .iter()
        .find_map(|suffix| lower.strip_suffix(suffix))
        .unwrap_or(&lower)
        .to_owned()
}
