//! `packageImportPatterns`: an install that imports only some of each package's files.

use crate::_utils::enable_gvs_in_workspace_yaml;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

fn pnpm(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// A project that depends on `@pnpm.e2e/pkg-with-1-dep`, whose packages each hold a
/// `package.json`, an `index.js` and a `LICENSE`, with `settings` added to its `pnpm-workspace.yaml`.
fn project(settings: &str) -> (CommandTempCwd<AddMockedRegistry>, PathBuf) {
    let cwd = CommandTempCwd::init().add_mocked_registry();
    let workspace = cwd.workspace.clone();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } }).to_string(
        ),
    )
    .expect("write package.json");
    let mut yaml = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(workspace.join("pnpm-workspace.yaml"))
        .expect("open pnpm-workspace.yaml");
    write!(yaml, "\n{settings}").expect("add the settings");
    (cwd, workspace)
}

/// Every regular file under `node_modules/.pnpm`, as `<slot>/<path>`, links not followed.
fn imported_files(workspace: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("read a directory") {
            let entry = entry.expect("read an entry");
            let kind = entry.file_type().expect("read an entry's type");
            if kind.is_dir() {
                walk(&entry.path(), base, out);
            } else if kind.is_file() {
                let path = entry.path();
                out.push(
                    path.strip_prefix(base)
                        .expect("under the base")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let base = workspace.join("node_modules/.pnpm");
    let mut out = Vec::new();
    for slot in fs::read_dir(&base).expect("read the virtual store") {
        let slot = slot.expect("read a slot");
        // `.pnpm/node_modules` holds the hoisted links, not a package slot.
        if slot
            .file_type()
            .expect("read a slot's type")
            .is_dir()
            && slot.file_name() != "node_modules"
        {
            walk(&slot.path().join("node_modules"), &base, &mut out);
        }
    }
    out.sort();
    out
}

fn file_names(files: &[String]) -> Vec<&str> {
    let mut names: Vec<&str> = files
        .iter()
        .map(|file| file.rsplit('/').next().unwrap_or(file))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[test]
fn without_patterns_every_package_file_is_imported() {
    let (_cwd, workspace) = project("");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(file_names(&imported_files(&workspace)), ["LICENSE", "index.js", "package.json"]);
}

#[test]
fn only_the_files_the_patterns_name_are_imported_with_each_package_json() {
    let (_cwd, workspace) = project("packageImportPatterns:\n  - '*.d.ts'\n");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let files = imported_files(&workspace);
    assert_eq!(file_names(&files), ["package.json"], "{files:?}");
    assert!(files.len() >= 2, "both packages are there: {files:?}");
}

#[test]
fn a_file_that_matches_a_pattern_is_imported() {
    let (_cwd, workspace) = project("packageImportPatterns:\n  - '*.js'\n");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(file_names(&imported_files(&workspace)), ["index.js", "package.json"]);
}

#[test]
fn an_install_from_the_lockfile_imports_the_same_files() {
    let (_cwd, workspace) = project("packageImportPatterns:\n  - '*.js'\n");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(file_names(&imported_files(&workspace)), ["index.js", "package.json"]);
}

#[test]
fn an_excluding_pattern_takes_a_file_back_out() {
    let (_cwd, workspace) = project("packageImportPatterns:\n  - '*'\n  - '!index.js'\n");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(file_names(&imported_files(&workspace)), ["LICENSE", "package.json"]);
}

#[test]
fn patterns_are_refused_with_the_global_virtual_store() {
    let (_cwd, workspace) = project("");
    enable_gvs_in_workspace_yaml(&workspace, "packageImportPatterns:\n  - '*.d.ts'\n");
    let output = pnpm(&workspace)
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{stdout}\n{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains(
            "ERR_PNPM_CONFIG_CONFLICT_PACKAGE_IMPORT_PATTERNS_WITH_GLOBAL_VIRTUAL_STORE"
        ),
        "{stdout}\n{stderr}",
    );
}
