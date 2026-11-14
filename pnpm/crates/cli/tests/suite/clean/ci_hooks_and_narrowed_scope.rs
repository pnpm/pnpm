use super::{CommandTempCwd, seed_package};
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, path::Path, process::Command};

fn run_pnpm(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(cwd)
        .without_ambient_pnpm_config()
        .with_args(args)
        .output()
        .expect("run pnpm")
}

fn write_local_dep(workspace: &Path, name: &str) {
    let dep_dir = workspace.join("deps").join(name);
    fs::create_dir_all(&dep_dir).expect("create local dep dir");
    fs::write(
        dep_dir.join("package.json"),
        serde_json::json!({ "name": name, "version": "1.0.0" }).to_string(),
    )
    .expect("write local dep manifest");
}

fn write_shared_workspace_with_root_and_member(workspace: &Path, store_dir: &Path) {
    write_local_dep(workspace, "dep-a");
    write_local_dep(workspace, "dep-b");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nstoreDir: {}\n",
            serde_json::to_string(store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"root-pkg","version":"1.0.0","private":true,"dependencies":{"dep-a":"file:deps/dep-a"}}"#,
    )
    .expect("write root manifest");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::write(
        pkg_a.join("package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"dep-b":"file:../../deps/dep-b"}}"#,
    )
    .expect("write pkg-a manifest");
}

#[test]
fn narrowed_root_clean_and_ci_preserve_shared_virtual_store_for_other_members() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_shared_workspace_with_root_and_member(&workspace, &store_dir);

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");
    let member_dep = workspace.join("packages/pkg-a/node_modules/dep-b/package.json");
    assert!(member_dep.exists(), "member dep installed into shared .pnpm");

    let root_clean = run_pnpm(&workspace, &["-w", "clean"]);
    assert!(root_clean.status.success(), "{root_clean:?}");
    assert!(!workspace.join("node_modules/dep-a").exists(), "root direct link removed");
    assert!(workspace.join("node_modules/.pnpm").exists(), "shared .pnpm preserved");
    assert!(workspace.join("node_modules/.modules.yaml").exists(), ".modules.yaml preserved");
    assert!(member_dep.exists(), "unselected member symlink into .pnpm still resolves");

    let reinstall = run_pnpm(&workspace, &["install"]);
    assert!(reinstall.status.success(), "{reinstall:?}");
    assert!(
        workspace.join("node_modules/dep-a/package.json").exists(),
        "plain install after -w clean restores root direct link",
    );

    let root_clean_again = run_pnpm(&workspace, &["-w", "clean"]);
    assert!(root_clean_again.status.success(), "{root_clean_again:?}");
    seed_package(&workspace.join("node_modules"), "stale-root");
    let root_ci = run_pnpm(&workspace, &["-w", "ci"]);
    assert!(root_ci.status.success(), "{root_ci:?}");
    assert!(!workspace.join("node_modules/stale-root").exists(), "stale root entry removed");
    assert!(workspace.join("node_modules/dep-a/package.json").exists(), "root dep reinstalled");
    assert!(member_dep.exists(), "unselected member symlink still resolves after -w ci");

    drop(root);
}

#[test]
fn clean_no_recursive_narrows_to_current_project_and_preserves_shared_virtual_store() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_shared_workspace_with_root_and_member(&workspace, &store_dir);

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");

    let pkg_a = workspace.join("packages/pkg-a");
    let sub_clean = run_pnpm(&pkg_a, &["clean", "--no-recursive"]);
    assert!(sub_clean.status.success(), "{sub_clean:?}");
    assert!(!pkg_a.join("node_modules/dep-b").exists(), "current member cleaned");
    assert!(workspace.join("node_modules/dep-a/package.json").exists(), "root untouched");
    assert!(workspace.join("node_modules/.pnpm").exists(), "shared .pnpm preserved");

    let reinstall = run_pnpm(&workspace, &["install"]);
    assert!(reinstall.status.success(), "{reinstall:?}");
    let root_clean = run_pnpm(&workspace, &["clean", "--no-recursive"]);
    assert!(root_clean.status.success(), "{root_clean:?}");
    assert!(!workspace.join("node_modules/dep-a").exists(), "root direct link cleaned");
    assert!(pkg_a.join("node_modules/dep-b/package.json").exists(), "member link resolves");
    assert!(workspace.join("node_modules/.pnpm").exists(), "shared .pnpm preserved");

    drop(root);
}

fn write_logging_pnpmfile(workspace: &Path, log_name: &str, version_comment: &str) {
    let script = format!(
        "// {version_comment}\n\
         const fs = require('node:fs');\n\
         const path = require('node:path');\n\
         const logFile = path.join(__dirname, {log_name:?});\n\
         module.exports = {{\n\
           hooks: {{\n\
             readPackage(pkg) {{\n\
               fs.appendFileSync(logFile, `read:${{pkg.name}}\\n`);\n\
               return pkg;\n\
             }},\n\
             preResolution() {{\n\
               fs.appendFileSync(logFile, 'preResolution\\n');\n\
             }},\n\
           }},\n\
         }};\n",
    );
    fs::write(workspace.join(".pnpmfile.cjs"), script).expect("write .pnpmfile.cjs");
}

#[test]
fn ci_executes_pnpmfile_hooks_once_and_skips_them_when_lockfile_or_checksum_fails() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_local_dep(&workspace, "dep-a");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("storeDir: {}\n", serde_json::to_string(&store_dir).expect("quote storeDir")),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"dep-a":"file:deps/dep-a"}}"#,
    )
    .expect("write package.json");
    write_logging_pnpmfile(&workspace, "hooks.log", "v1");

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");
    let log_path = workspace.join("hooks.log");
    fs::remove_file(&log_path).expect("reset hooks.log");

    let ok_ci = run_pnpm(&workspace, &["ci"]);
    assert!(ok_ci.status.success(), "{ok_ci:?}");
    let lines: Vec<String> = fs::read_to_string(&log_path)
        .expect("read hooks.log")
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "read:app")
            .count(),
        1,
        "{lines:?}",
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "preResolution")
            .count(),
        1,
        "{lines:?}",
    );

    assert_drifted_or_missing_lockfile_skips_hooks(&workspace, &log_path);
    drop(root);
}

fn assert_drifted_or_missing_lockfile_skips_hooks(workspace: &Path, log_path: &Path) {
    let marker = workspace.join("node_modules/keep-marker.txt");
    fs::write(&marker, "keep").expect("write marker");
    let _ = fs::remove_file(log_path);

    write_logging_pnpmfile(workspace, "hooks.log", "v2-drifted");
    let drifted = run_pnpm(workspace, &["ci"]);
    assert!(!drifted.status.success(), "drifted pnpmfileChecksum must fail");
    let drifted_stderr = String::from_utf8_lossy(&drifted.stderr);
    assert!(drifted_stderr.contains("ERR_PNPM_LOCKFILE_CONFIG_MISMATCH"), "{drifted_stderr}");
    assert!(!log_path.exists(), "hooks must not run when pnpmfileChecksum mismatches");
    assert!(marker.exists(), "node_modules must survive drifted pnpmfileChecksum");

    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove lockfile");
    let missing = run_pnpm(workspace, &["ci"]);
    assert!(!missing.status.success(), "missing lockfile must fail");
    assert!(!log_path.exists(), "hooks must not run when lockfile is missing");
    assert!(marker.exists(), "node_modules must survive missing lockfile");
}

#[test]
fn ci_rejects_missing_snapshot_entry_before_cleaning_node_modules() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"dep-a":"1.0.0"}}"#,
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n\n\
         importers:\n  .:\n    dependencies:\n      dep-a:\n        specifier: 1.0.0\n        version: 1.0.0\n\n\
         packages:\n  dep-a@1.0.0:\n    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}\n\n\
         snapshots: {}\n",
    )
    .expect("write broken lockfile");
    seed_package(&workspace.join("node_modules"), "keep-pkg");

    let ci_out = run_pnpm(&workspace, &["ci"]);
    assert!(!ci_out.status.success(), "ci with missing snapshot must fail");
    let stderr = String::from_utf8_lossy(&ci_out.stderr);
    assert!(stderr.contains("ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY"), "got: {stderr}");
    assert!(
        workspace.join("node_modules/keep-pkg/package.json").exists(),
        "node_modules must survive missing snapshot validation failure",
    );

    drop(root);
}

#[test]
fn clean_and_ci_honor_per_package_modules_dir_in_package_configs() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_local_dep(&workspace, "dep-a");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\n\
             sharedWorkspaceLockfile: false\n\
             storeDir: {}\n\
             packageConfigs:\n  pkg-a:\n    modulesDir: custom_nm\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::write(
        pkg_a.join("package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"dep-a":"file:../../deps/dep-a"}}"#,
    )
    .expect("write pkg-a manifest");

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");
    assert!(pkg_a.join("custom_nm/dep-a/package.json").exists(), "installed into custom_nm");

    seed_package(&pkg_a.join("custom_nm"), "stale-pkg");
    let ci_out = run_pnpm(&workspace, &["ci"]);
    assert!(ci_out.status.success(), "{ci_out:?}");
    assert!(!pkg_a.join("custom_nm/stale-pkg").exists(), "ci cleaned custom_nm");
    assert!(pkg_a.join("custom_nm/dep-a/package.json").exists(), "ci reinstalled custom_nm");

    let clean_out = run_pnpm(&workspace, &["clean"]);
    assert!(clean_out.status.success(), "{clean_out:?}");
    assert!(!pkg_a.join("custom_nm/dep-a").exists(), "clean emptied custom_nm");

    drop(root);
}

#[test]
fn ci_filter_passes_existing_current_lockfile_to_pre_resolution_while_unfiltered_ci_clears_it() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_shared_workspace_with_root_and_member(&workspace, &store_dir);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "const fs = require('node:fs');\n\
         const path = require('node:path');\n\
         module.exports = {\n\
           hooks: {\n\
             preResolution(ctx) {\n\
               const hasPackages = Boolean(ctx.currentLockfile?.packages && Object.keys(ctx.currentLockfile.packages).length > 0);\n\
               fs.writeFileSync(path.join(__dirname, 'pre-res.json'), JSON.stringify({ exists: ctx.existsCurrentLockfile, hasPackages }));\n\
             },\n\
           },\n\
         };\n",
    )
    .expect("write .pnpmfile.cjs");
    assert!(run_pnpm(&workspace, &["install"]).status.success(), "initial install");

    let filtered = run_pnpm(&workspace, &["ci", "--filter", "pkg-a"]);
    assert!(filtered.status.success(), "{filtered:?}");
    let filtered_obs: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("pre-res.json")).expect("read"))
            .expect("parse filtered pre-res.json");
    assert_eq!(filtered_obs["exists"], serde_json::json!(true), "{filtered_obs}");
    assert_eq!(filtered_obs["hasPackages"], serde_json::json!(true), "{filtered_obs}");

    let unfiltered = run_pnpm(&workspace, &["ci"]);
    assert!(unfiltered.status.success(), "{unfiltered:?}");
    let unfiltered_obs: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("pre-res.json")).expect("read"))
            .expect("parse unfiltered pre-res.json");
    assert_eq!(unfiltered_obs["exists"], serde_json::json!(false), "{unfiltered_obs}");

    drop(root);
}

#[test]
fn clean_preserves_members_when_rootless_workspace_sets_modules_dir_to_member_parent() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nmodulesDir: packages\n",
    )
    .expect("write pnpm-workspace.yaml");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(pkg_a.join("src")).expect("create pkg-a/src");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#)
        .expect("write pkg-a manifest");
    fs::write(pkg_a.join("src/index.js"), "module.exports = 1;\n").expect("write pkg-a source");

    for args in [&["clean", "--filter", "pkg-a"][..], &["clean"][..]] {
        seed_package(&pkg_a.join("packages"), "seeded-dep");
        let out = run_pnpm(&workspace, args);
        assert!(out.status.success(), "{args:?} failed: {out:?}");
        assert!(!pkg_a.join("packages/seeded-dep").exists(), "{args:?} removed member modulesDir");
        assert!(pkg_a.join("package.json").exists(), "{args:?} preserved member manifest");
        assert!(pkg_a.join("src/index.js").exists(), "{args:?} preserved member source");
    }

    drop(root);
}

#[test]
fn clean_filter_root_with_dot_lockfile_dir_preserves_shared_virtual_store() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_shared_workspace_with_root_and_member(&workspace, &store_dir);
    assert!(run_pnpm(&workspace, &["install"]).status.success(), "initial install");

    let clean_out = run_pnpm(&workspace, &["clean", "--filter", "root-pkg", "--lockfile-dir", "."]);
    assert!(clean_out.status.success(), "{clean_out:?}");
    assert!(!workspace.join("node_modules/dep-a").exists(), "root direct link removed");
    assert!(workspace.join("node_modules/.pnpm").exists(), "shared .pnpm preserved");
    assert!(workspace.join("node_modules/.modules.yaml").exists(), ".modules.yaml preserved");
    assert!(
        workspace.join("packages/pkg-a/node_modules/dep-b/package.json").exists(),
        "unselected member link into .pnpm still resolves",
    );

    drop(root);
}
