use command_extra::CommandExtra;
use std::fs;
use std::path::Path;

use pacquet_testing_utils::bin::CommandTempCwd;

/// Create `dir/node_modules/<name>/package.json` so the directory
/// looks like an installed package that `clean` must remove.
fn seed_package(node_modules: &Path, name: &str) {
    let pkg_dir = node_modules.join(name);
    fs::create_dir_all(&pkg_dir).expect("create package dir");
    fs::write(pkg_dir.join("package.json"), "{}").expect("write package manifest");
}

#[test]
fn clean_removes_packages_and_pnpm_entries_but_preserves_non_pnpm_dotfiles() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let node_modules = workspace.join("node_modules");
    fs::create_dir_all(&node_modules).expect("create node_modules");
    seed_package(&node_modules, "lodash");
    fs::create_dir_all(node_modules.join(".pnpm")).expect("create .pnpm");
    fs::create_dir_all(node_modules.join(".cache")).expect("create .cache");
    fs::write(node_modules.join(".cache").join("data"), "x").expect("write .cache/data");

    let output = pacquet.with_args(["clean"]).output().expect("run pacquet clean");
    assert!(output.status.success(), "pacquet clean should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removing node_modules"), "expected Removing node_modules: {stdout}");

    // Regular packages and pnpm hidden entries are gone.
    assert!(!node_modules.join("lodash").exists(), "lodash package should be removed");
    assert!(!node_modules.join(".pnpm").exists(), ".pnpm should be removed");

    // Non-pnpm dotfiles (e.g. .cache) are preserved, along with the
    // node_modules directory itself.
    assert!(node_modules.join(".cache").exists(), ".cache should be preserved");
    assert!(node_modules.exists(), "node_modules directory should remain");

    drop(root);
}

#[test]
fn clean_handles_missing_node_modules_gracefully() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_args(["clean"]).output().expect("run pacquet clean");
    assert!(output.status.success(), "pacquet clean should succeed with no node_modules");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "nothing should be printed: {stdout}");

    drop(root);
}

#[test]
fn clean_preserves_lockfile_by_default() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let lockfile = workspace.join("pnpm-lock.yaml");
    fs::write(&lockfile, "lockfileVersion: '9.0'\n").expect("write lockfile");

    let output = pacquet.with_args(["clean"]).output().expect("run pacquet clean");
    assert!(output.status.success(), "pacquet clean should succeed");

    assert!(lockfile.exists(), "pnpm-lock.yaml should be preserved by default");

    drop(root);
}

#[test]
fn clean_lockfile_removes_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let lockfile = workspace.join("pnpm-lock.yaml");
    fs::write(&lockfile, "lockfileVersion: '9.0'\n").expect("write lockfile");

    let output =
        pacquet.with_args(["clean", "--lockfile"]).output().expect("run pacquet clean --lockfile");
    assert!(output.status.success(), "pacquet clean --lockfile should succeed");

    assert!(!lockfile.exists(), "pnpm-lock.yaml should be removed with --lockfile");

    drop(root);
}

#[test]
fn clean_works_in_a_workspace() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - pkg1\n  - pkg2\n")
        .expect("write pnpm-workspace.yaml");
    let pkg1 = workspace.join("pkg1");
    let pkg2 = workspace.join("pkg2");
    fs::create_dir_all(&pkg1).expect("create pkg1");
    fs::create_dir_all(&pkg2).expect("create pkg2");
    fs::write(pkg1.join("package.json"), "{}").expect("write pkg1 manifest");
    fs::write(pkg2.join("package.json"), "{}").expect("write pkg2 manifest");
    seed_package(&pkg1.join("node_modules"), "a");
    seed_package(&pkg2.join("node_modules"), "b");

    let output = pacquet.with_args(["clean"]).output().expect("run pacquet clean");
    assert!(output.status.success(), "pacquet clean should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removing pkg1/node_modules"), "expected pkg1: {stdout}");
    assert!(stdout.contains("Removing pkg2/node_modules"), "expected pkg2: {stdout}");
    assert!(!pkg1.join("node_modules").join("a").exists(), "pkg1 package removed");
    assert!(!pkg2.join("node_modules").join("b").exists(), "pkg2 package removed");

    drop(root);
}

#[test]
fn clean_removes_custom_virtual_store_dir_inside_the_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "virtualStoreDir: .pnpm-store\npackages:\n  - .\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), "{}").expect("write root manifest");
    let node_modules = workspace.join("node_modules");
    fs::create_dir_all(&node_modules).expect("create node_modules");
    seed_package(&node_modules, "lodash");
    let virtual_store = workspace.join(".pnpm-store");
    fs::create_dir_all(&virtual_store).expect("create custom virtual store");

    let output = pacquet.with_args(["clean"]).output().expect("run pacquet clean");
    assert!(output.status.success(), "pacquet clean should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removing .pnpm-store"), "expected custom store removal: {stdout}");
    assert!(!virtual_store.exists(), "custom virtual store should be removed");
    assert!(!node_modules.join("lodash").exists(), "packages removed");

    drop(root);
}

#[test]
fn clean_does_not_remove_virtual_store_dir_outside_the_project_root() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let outside = workspace.parent().unwrap().join("outside-store");
    fs::create_dir_all(&outside).expect("create store outside root");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "virtualStoreDir: ../outside-store\npackages:\n  - .\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), "{}").expect("write root manifest");
    let node_modules = workspace.join("node_modules");
    fs::create_dir_all(&node_modules).expect("create node_modules");
    seed_package(&node_modules, "lodash");

    let output = pacquet.with_args(["clean"]).output().expect("run pacquet clean");
    assert!(output.status.success(), "pacquet clean should succeed");

    assert!(outside.exists(), "virtual store outside root must be left alone");

    drop(root);
}

/// Script-override tests drive `/bin/sh` lifecycle scripts, so the whole
/// module is Unix-only.
#[cfg(unix)]
mod scripts {
    use super::*;

    fn write_manifest(dir: &Path, scripts: &serde_json::Value) {
        let manifest = serde_json::json!({
            "name": "test-pkg",
            "version": "1.0.0",
            "scripts": scripts,
        });
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }

    #[test]
    fn clean_runs_the_clean_script_when_present() {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

        write_manifest(&workspace, &serde_json::json!({ "clean": "echo script-clean-ran" }));
        let node_modules = workspace.join("node_modules");
        fs::create_dir_all(&node_modules).expect("create node_modules");
        seed_package(&node_modules, "lodash");

        let output = pacquet.with_args(["clean"]).output().expect("run pacquet clean");
        assert!(output.status.success(), "pacquet clean should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("script-clean-ran"), "clean script should run: {stdout}");
        // The built-in clean is overridden, so node_modules is untouched.
        assert!(
            node_modules.join("lodash").exists(),
            "node_modules must not be cleaned when a script overrides"
        );

        drop(root);
    }

    #[test]
    fn purge_runs_the_builtin_clean_when_only_a_clean_script_exists() {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

        write_manifest(&workspace, &serde_json::json!({ "clean": "echo script-clean-ran" }));
        let node_modules = workspace.join("node_modules");
        fs::create_dir_all(&node_modules).expect("create node_modules");
        seed_package(&node_modules, "lodash");

        let output = pacquet.with_args(["purge"]).output().expect("run pacquet purge");
        assert!(output.status.success(), "pacquet purge should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.contains("script-clean-ran"),
            "purge must not run the clean script: {stdout}"
        );
        assert!(!node_modules.join("lodash").exists(), "purge built-in must clean node_modules");

        drop(root);
    }

    #[test]
    fn purge_runs_the_purge_script_when_present() {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

        write_manifest(&workspace, &serde_json::json!({ "purge": "echo script-purge-ran" }));
        let node_modules = workspace.join("node_modules");
        fs::create_dir_all(&node_modules).expect("create node_modules");
        seed_package(&node_modules, "lodash");

        let output = pacquet.with_args(["purge"]).output().expect("run pacquet purge");
        assert!(output.status.success(), "pacquet purge should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("script-purge-ran"), "purge script should run: {stdout}");
        assert!(
            node_modules.join("lodash").exists(),
            "node_modules must not be cleaned when a script overrides"
        );

        drop(root);
    }
}
