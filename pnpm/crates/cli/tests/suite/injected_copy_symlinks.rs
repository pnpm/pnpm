//! An injected copy holds the package's links to its own content the way
//! the install writes them, after a script listed in
//! `syncInjectedDepsAfterScripts` too (pnpm/tasks#147).

#![cfg(unix)]

use crate::_utils::{WorkspaceFixture, assert_success};

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use serde_json::json;
use walkdir::WalkDir;

/// The build replaces a file under a linked directory and adds a new link.
const BUILD: &str = "const fs = require('fs')\n\
fs.rmSync(__dirname + '/files/foo/a.txt')\n\
fs.writeFileSync(__dirname + '/files/foo/a.txt', 'rebuilt')\n\
fs.symlinkSync('files/foo', __dirname + '/new-link')\n";

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Dir,
    File(String),
    Link(PathBuf),
}

/// `app` injects `lib`, which links to its own directories: two links to
/// one directory, and a link to the package root.
fn workspace(deploy_all_files: bool) -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "deployAllFiles: {deploy_all_files}\ndedupeInjectedDeps: false\n\
         syncInjectedDepsAfterScripts:\n  - build\n",
    ));
    let write = |path: &str, contents: &str| {
        let path = fixture.workspace.join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("create a directory");
        fs::write(path, contents).expect("write a file");
    };
    write("package.json", &json!({ "name": "root", "private": true }).to_string());
    write(
        "packages/app/package.json",
        &json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*" },
            "dependenciesMeta": { "lib": { "injected": true } },
        })
        .to_string(),
    );
    write(
        "packages/lib/package.json",
        &json!({ "name": "lib", "version": "1.0.0", "scripts": { "build": "node build.cjs" } })
            .to_string(),
    );
    write("packages/lib/build.cjs", BUILD);
    write("packages/lib/files/foo/a.txt", "a");
    write("packages/lib/shared/x.txt", "x");
    let lib = fixture.workspace.join("packages/lib");
    for (target, link) in
        [("files/foo", "link-to-a-dir"), ("shared", "a"), ("shared", "b"), (".", "loop")]
    {
        symlink(target, lib.join(link)).expect("create a link");
    }
    fixture
}

/// The copy of `lib` that `app` resolves in `workspace`.
fn injected_copy(workspace: &Path) -> PathBuf {
    let copy = fs::canonicalize(workspace.join("packages/app/node_modules/lib"))
        .expect("app resolves lib");
    let source = fs::canonicalize(workspace.join("packages/lib")).expect("canonicalize lib");
    assert_ne!(copy, source, "lib is linked, not injected");
    copy
}

/// Every entry under `dir` outside `node_modules`, without following links.
fn tree(dir: &Path) -> BTreeMap<String, Entry> {
    WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "node_modules")
        .map(|entry| entry.expect("walk the copy"))
        .map(|entry| {
            let relative = entry
                .path()
                .strip_prefix(dir)
                .expect("under the copy")
                .to_string_lossy()
                .into_owned();
            let kind = if entry.path_is_symlink() {
                Entry::Link(fs::read_link(entry.path()).expect("read a link"))
            } else if entry.file_type().is_dir() {
                Entry::Dir
            } else {
                Entry::File(fs::read_to_string(entry.path()).expect("read a file"))
            };
            (relative, kind)
        })
        .collect()
}

/// A copy of the sources under `from`, links kept as links.
fn copy_keeping_links(from: &Path, to: &Path) {
    for entry in WalkDir::new(from)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "node_modules")
    {
        let entry = entry.expect("walk the workspace");
        let target = to.join(
            entry
                .path()
                .strip_prefix(from)
                .expect("under the workspace"),
        );
        if entry.path_is_symlink() {
            symlink(fs::read_link(entry.path()).expect("read a link"), &target)
                .expect("copy a link");
        } else if entry.file_type().is_dir() {
            fs::create_dir_all(&target).expect("create a directory");
        } else {
            fs::copy(entry.path(), &target).expect("copy a file");
        }
    }
}

fn check(deploy_all_files: bool) {
    let fixture = workspace(deploy_all_files);
    let workspace = fixture.workspace.clone();
    assert_success(&fixture.command_at(&workspace, ["install"]));
    assert_success(&fixture.command_at(&workspace.join("packages/lib"), ["run", "build"]));

    let clean = workspace.with_file_name("clean");
    copy_keeping_links(&workspace, &clean);
    assert_success(&fixture.command_at(&clean, ["install", "--frozen-lockfile"]));

    assert_eq!(
        tree(&injected_copy(&workspace)),
        tree(&injected_copy(&clean)),
        "deployAllFiles: {deploy_all_files}: the synced copy differs from the one a fresh install writes",
    );
}

#[test]
fn a_listed_script_keeps_links_as_the_install_writes_them() {
    check(false);
}

#[test]
fn a_listed_script_keeps_links_as_the_install_writes_them_with_deploy_all_files() {
    check(true);
}
