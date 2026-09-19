#![cfg(unix)]

use crate::{
    _utils::{ManifestDeps, append_workspace_yaml_key, pacquet_in, write_project_manifest},
    repeat_install::version_of,
};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{MTIME_STEP_MS, set_mtime_ms},
};
use pnpm_workspace_state::{WORKSPACE_STATE_FILENAME, load_workspace_state};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

/// The inode and the mtime and ctime timestamps of every entry under a
/// root, symlinks included, which together show any write an install makes.
type TreeSnapshot = BTreeMap<PathBuf, (u64, i64, i64, i64, i64)>;

fn tree_snapshot(root: &Path) -> TreeSnapshot {
    WalkDir::new(root)
        .into_iter()
        .map(|entry| {
            let entry = entry.expect("walk the tree");
            let metadata = entry.metadata().expect("lstat the entry");
            let stamps = (
                metadata.ino(),
                metadata.mtime(),
                metadata.mtime_nsec(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            );
            (entry.into_path(), stamps)
        })
        .collect()
}

/// The entries of `root` that differ between two [`tree_snapshot`]s.
fn changed_entries(before: &TreeSnapshot, after: &TreeSnapshot, root: &Path) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .map(|path| {
            path.strip_prefix(root)
                .expect("an entry under root")
                .display()
                .to_string()
        })
        .collect()
}

/// The files under `root` whose bytes mention `path`.
fn files_mentioning(root: &Path, path: &Path) -> Vec<PathBuf> {
    let needle = path.to_string_lossy().into_owned();
    WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.expect("walk the tree"))
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            let bytes = fs::read(entry.path()).expect("read the file");
            String::from_utf8_lossy(&bytes).contains(&needle)
        })
        .map(walkdir::DirEntry::into_path)
        .collect()
}

/// A workspace whose relative store and cache are pinned to the same
/// directories by their canonical absolute paths, so nothing in the tree
/// names the store through the project dir and a move to another depth
/// keeps the store. Returns the temp dirs to keep alive for the test and
/// the canonical workspace dir.
fn pinned_workspace() -> (CommandTempCwd<AddMockedRegistry>, PathBuf) {
    let temp_cwd = CommandTempCwd::init().add_mocked_registry();
    let root = fs::canonicalize(temp_cwd.root.path()).expect("canonicalize the test root");
    for config_file in [".npmrc", "pnpm-workspace.yaml"] {
        let path = temp_cwd.workspace.join(config_file);
        let text = fs::read_to_string(&path)
            .expect("read the store config")
            .replace("../pacquet-store", &root.join("pacquet-store").to_string_lossy())
            .replace("../pacquet-cache", &root.join("pacquet-cache").to_string_lossy());
        fs::write(&path, text).expect("write the store config");
    }
    let workspace = fs::canonicalize(&temp_cwd.workspace).expect("canonicalize the workspace");
    (temp_cwd, workspace)
}

#[test]
fn copied_workspace_is_up_to_date_after_one_state_write() {
    for node_linker in ["isolated", "hoisted"] {
        let (temp_cwd, workspace) = pinned_workspace();
        append_workspace_yaml_key(&workspace, "nodeLinker", node_linker);
        pacquet_in(&workspace)
            .with_args(["add", "@pnpm.e2e/hello-world-js-bin-parent@1.0.0"])
            .assert()
            .success();
        let copy = workspace.with_file_name("copy");
        Command::new("cp")
            .arg("-Rp")
            .arg(&workspace)
            .arg(&copy)
            .assert()
            .success();
        let bin = copy.join("node_modules/.bin/hello-world-js-bin");
        assert!(bin.is_file(), "{node_linker}: the fixture must contain a bin");
        let before = tree_snapshot(&copy);

        let first = pacquet_in(&copy)
            .with_arg("install")
            .assert()
            .success();
        let first_output = String::from_utf8_lossy(&first.get_output().stdout).into_owned();
        assert!(
            first_output.contains("Already up to date")
                && !first_output.contains("resolution step is skipped"),
            "the copy must be up to date before the install pipeline runs: {first_output}",
        );
        assert_state_recorded_at(&copy);
        let after_first = tree_snapshot(&copy);
        assert_eq!(
            changed_entries(&before, &after_first, &copy),
            BTreeSet::from([
                "node_modules".to_string(),
                format!("node_modules/{WORKSPACE_STATE_FILENAME}"),
            ]),
        );
        pacquet_in(&copy)
            .with_arg("install")
            .assert()
            .success();
        assert_eq!(changed_entries(&after_first, &tree_snapshot(&copy), &copy), BTreeSet::new());

        let run = Command::new(&bin).assert().success();
        let stdout = String::from_utf8_lossy(&run.get_output().stdout);
        assert!(
            stdout.contains("Hello world from hello-world-js-bin-parent!"),
            "{node_linker}: copied bin output: {stdout}",
        );

        drop(temp_cwd);
    }
}

#[test]
fn moved_rootless_workspace_reuses_its_sibling_projects() {
    for node_linker in ["isolated", "hoisted"] {
        let (temp_cwd, workspace) = pinned_workspace();
        append_workspace_yaml_key(&workspace, "nodeLinker", node_linker);
        append_workspace_yaml_key(&workspace, "packages", "[packages/*]");
        for project in ["a", "b"] {
            write_project_manifest(
                &workspace.join("packages").join(project),
                project,
                ManifestDeps { prod: &[("is-positive", "1.0.0")], ..ManifestDeps::default() },
            );
        }
        assert!(!workspace.join("package.json").exists());
        pacquet_in(&workspace)
            .with_arg("install")
            .assert()
            .success();
        let moved = workspace.with_file_name("moved-rootless");
        fs::rename(&workspace, &moved).expect("move the rootless workspace");
        let before = tree_snapshot(&moved);
        let install = pacquet_in(&moved)
            .with_arg("install")
            .assert()
            .success();
        let output = String::from_utf8_lossy(&install.get_output().stdout);
        assert!(
            output.contains("Already up to date") && !output.contains("resolution step is skipped"),
            "{node_linker}: the rootless move must reuse the installation: {output}",
        );
        assert_eq!(
            changed_entries(&before, &tree_snapshot(&moved), &moved),
            BTreeSet::from([
                "node_modules".to_string(),
                format!("node_modules/{WORKSPACE_STATE_FILENAME}"),
            ]),
        );
        let state = load_workspace_state(&moved).unwrap().unwrap();
        let expected = ["a", "b"].map(|project| {
            moved
                .join("packages")
                .join(project)
                .to_string_lossy()
                .into_owned()
        });
        assert_eq!(
            state.projects
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected.into(),
        );
        let settled = tree_snapshot(&moved);
        pacquet_in(&moved)
            .with_arg("install")
            .assert()
            .success();
        assert_eq!(changed_entries(&settled, &tree_snapshot(&moved), &moved), BTreeSet::new());
        drop(temp_cwd);
    }
}

/// A `node_modules` copied into another checkout of the project, whose
/// lockfile pins another version, is not the tree that checkout wants, even
/// though every file of the checkout predates the state it carries.
#[test]
fn transplanted_node_modules_is_not_up_to_date() {
    let (temp_cwd, workspace) = pinned_workspace();
    pacquet_in(&workspace)
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"])
        .assert()
        .success();
    let checkout = workspace.with_file_name("checkout");
    Command::new("cp")
        .arg("-Rp")
        .arg(&workspace)
        .arg(&checkout)
        .assert()
        .success();
    fs::write(
        checkout.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" } })
            .to_string(),
    )
    .expect("pin the checkout to another version");
    pacquet_in(&checkout)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let validated = load_workspace_state(&checkout)
        .expect("read the transplanted state")
        .expect("a transplanted state")
        .last_validated_timestamp;
    for file in ["package.json", "pnpm-lock.yaml"] {
        set_mtime_ms(&checkout.join(file), validated - 2 * MTIME_STEP_MS);
    }

    let install = pacquet_in(&checkout)
        .with_arg("install")
        .assert()
        .success();
    let output = String::from_utf8_lossy(&install.get_output().stdout).into_owned();
    assert!(!output.contains("Already up to date"), "the transplant must be relinked: {output}");
    assert_eq!(version_of(&checkout, "node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep"), "100.0.0");

    drop(temp_cwd);
}

/// The patch file [`install_patched_workspace`] configures.
const IS_POSITIVE_PATCH_FILE: &str = "patches/is-positive.patch";

/// An installed workspace whose `is-positive` carries a patch that adds a
/// marker file, with the store pinned outside it.
fn install_patched_workspace() -> (CommandTempCwd<AddMockedRegistry>, PathBuf) {
    let (temp_cwd, workspace) = pinned_workspace();
    fs::create_dir(workspace.join("patches")).expect("create the patches dir");
    fs::write(workspace.join(IS_POSITIVE_PATCH_FILE), crate::patch::MARKER_PATCH)
        .expect("write the patch");
    append_workspace_yaml_key(
        &workspace,
        "patchedDependencies",
        format!("\n  is-positive@1.0.0: {IS_POSITIVE_PATCH_FILE}"),
    );
    pacquet_in(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    (temp_cwd, workspace)
}

/// A plain `cp -R` stamps fresh mtimes on the copy, the patch file
/// included, so the first install in it proves the move by content instead
/// of by the patch mtimes, and the install after it writes nothing. The
/// patch hashes the lockfile records are what refuses the same tree once
/// its patch is edited.
#[test]
fn a_moved_patched_workspace_is_up_to_date_until_its_patch_is_edited() {
    let (temp_cwd, workspace) = install_patched_workspace();

    let copy = workspace.with_file_name("copy");
    Command::new("cp")
        .arg("-R")
        .arg(&workspace)
        .arg(&copy)
        .assert()
        .success();
    let first = pacquet_in(&copy)
        .with_arg("install")
        .assert()
        .success();
    let first_output = String::from_utf8_lossy(&first.get_output().stdout).into_owned();
    assert!(
        first_output.contains("Already up to date")
            && !first_output.contains("resolution step is skipped"),
        "the copy must be up to date before the install pipeline runs: {first_output}",
    );
    let moved = workspace.with_file_name("moved");
    fs::rename(&workspace, &moved).expect("move the workspace");
    fs::write(
        moved.join(IS_POSITIVE_PATCH_FILE),
        crate::patch::MARKER_PATCH.replace("+patched", "+edited"),
    )
    .expect("edit the patch");
    pacquet_in(&moved)
        .with_arg("install")
        .assert()
        .success();
    let marker = moved.join("node_modules/is-positive/patched-marker.txt");
    assert_eq!(fs::read_to_string(marker).expect("read the marker"), "edited\n");

    drop(temp_cwd);
}

/// The state under `dir` records only the project at `dir`.
fn assert_state_recorded_at(dir: &Path) {
    let state = load_workspace_state(dir).expect("read the state").expect("a workspace state");
    let keys: Vec<&String> = state.projects.keys().collect();
    dbg!(&keys);
    assert_eq!(keys, [&dir.to_string_lossy().into_owned()]);
}

/// Keep the harness's workspace manifest, so the install is a workspace
/// install. `dedupePeers` lands in the lockfile, which the proof of the
/// move compares with the configured value.
fn as_a_workspace(workspace: &Path) {
    append_workspace_yaml_key(workspace, "dedupePeers", true);
}

/// Drop the harness's workspace manifest, so the install is a lone project.
fn as_a_single_project(workspace: &Path) {
    fs::remove_file(workspace.join("pnpm-workspace.yaml")).expect("remove the workspace manifest");
}

#[test]
fn a_moved_project_runs_and_is_recorded_where_it_is_now() {
    let shapes =
        [("a workspace", as_a_workspace as fn(&Path)), ("one project", as_a_single_project)];
    for (label, shape) in shapes {
        let (temp_cwd, workspace) = pinned_workspace();
        shape(&workspace);
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({ "scripts": { "hello": "hello-world-js-bin" } }).to_string(),
        )
        .expect("write package.json");
        pacquet_in(&workspace)
            .with_args(["add", "@pnpm.e2e/hello-world-js-bin@1.0.0"])
            .assert()
            .success();

        let moved = workspace.with_file_name("moved");
        fs::rename(&workspace, &moved).expect("move the project");
        let run = pacquet_in(&moved)
            .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
            .assert()
            .success();
        let stdout = String::from_utf8_lossy(&run.get_output().stdout).into_owned();
        assert!(
            stdout.contains("Hello world!"),
            "{label}: the bin must run from {moved:?}: {stdout}",
        );
        assert_state_recorded_at(&moved);

        let moved_again = workspace.with_file_name("moved-again");
        fs::rename(&moved, &moved_again).expect("move the project again");
        pacquet_in(&moved_again)
            .with_arg("install")
            .assert()
            .success();
        assert_state_recorded_at(&moved_again);

        drop(temp_cwd);
    }
}

/// The injected-deps syncer links a package's own bins into the
/// `node_modules/.bin` beside it in its slot. A bin there that names where
/// the tree was refuses the move like one in any other `.bin`.
#[test]
fn moved_tree_with_a_stale_slot_bin_is_not_up_to_date() {
    let (temp_cwd, workspace) = pinned_workspace();
    pacquet_in(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
    let slot_bin_dir = workspace.join("node_modules/.pnpm/is-positive@1.0.0/node_modules/.bin");
    fs::create_dir(&slot_bin_dir).expect("create the slot's bin dir");
    fs::write(
        slot_bin_dir.join("stale"),
        format!("#!/bin/sh\n# cmd-shim-target={}/node_modules/stale/cli.js\n", workspace.display()),
    )
    .expect("write a bin naming where the tree is");

    let moved = workspace.with_file_name("moved");
    fs::rename(&workspace, &moved).expect("move the workspace");
    let install = pacquet_in(&moved)
        .with_arg("install")
        .assert()
        .success();
    let output = String::from_utf8_lossy(&install.get_output().stdout).into_owned();
    assert!(
        output.contains("resolution step is skipped"),
        "the stale bin must refuse the move before the install pipeline runs: {output}",
    );

    drop(temp_cwd);
}

/// The shim for `@pnpm.e2e/hello-world-js-bin` in its parent's slot.
const SLOT_SHIM: &str = "node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin-parent@1.0.0/node_modules/@pnpm.e2e/hello-world-js-bin-parent/node_modules/.bin/hello-world-js-bin";

fn make_shims_absolute_and_move(location: &Path, name: &str) -> PathBuf {
    for shim in [
        "b/node_modules/.bin/hello-world-js-bin",
        "node_modules/.pnpm/node_modules/.bin/hello-world-js-bin",
        SLOT_SHIM,
    ] {
        let shim = location.join(shim);
        let shim_dir = format!(
            "{}/",
            shim.parent()
                .expect("the shim's dir")
                .display(),
        );
        let body = fs::read_to_string(&shim).expect("read the shim");
        assert!(body.contains("$basedir_abs/"), "a relocatable shim: {body}");
        fs::write(&shim, body.replace("$basedir_abs/", &shim_dir)).expect("write an absolute shim");
    }
    let moved = location.with_file_name(name);
    fs::rename(location, &moved).expect("move the workspace");
    moved
}

#[test]
fn moved_tree_with_absolute_shims_converges_after_one_unfiltered_install() {
    for state in ["valid", "missing", "invalid"] {
        check_absolute_shim_repair(state);
    }
}

fn check_absolute_shim_repair(state: &str) {
    let (temp_cwd, workspace) = pinned_workspace();
    append_workspace_yaml_key(&workspace, "packages", "[a, b]");
    write_project_manifest(&workspace, "root", ManifestDeps::default());
    for (project, dependency) in
        [("a", "is-positive"), ("b", "@pnpm.e2e/hello-world-js-bin-parent")]
    {
        let prod = [(dependency, "1.0.0")];
        let deps = ManifestDeps { prod: &prod, ..ManifestDeps::default() };
        write_project_manifest(&workspace.join(project), project, deps);
    }
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let moved = make_shims_absolute_and_move(&workspace, "first-move");
    let state_path = moved.join("node_modules").join(WORKSPACE_STATE_FILENAME);
    match state {
        "missing" => fs::remove_file(&state_path).expect("remove workspace state"),
        "invalid" => fs::write(&state_path, "{").expect("corrupt workspace state"),
        _ => {}
    }
    pacquet_in(&moved)
        .with_args(["--filter", "a", "install"])
        .assert()
        .success();
    pacquet_in(&moved)
        .with_arg("install")
        .assert()
        .success();
    let mentions = files_mentioning(&moved, &workspace);
    assert!(mentions.is_empty(), "files naming where the tree was: {mentions:?}");
    let relinked = tree_snapshot(&moved);
    pacquet_in(&moved)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(changed_entries(&relinked, &tree_snapshot(&moved), &moved), BTreeSet::new());

    let moved_again = make_shims_absolute_and_move(&moved, "second-move");
    pacquet_in(&moved_again)
        .with_args(["install", "--no-prefer-frozen-lockfile"])
        .assert()
        .success();
    let mentions = files_mentioning(&moved_again, &moved);
    assert!(mentions.is_empty(), "files naming where the tree was: {mentions:?}");

    drop(temp_cwd);
}
