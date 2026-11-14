use super::{CommandCargoExt, CommandExtra, CommandTempCwd, CommandTestExt, Path, fs};
use std::process::Command;

fn write_pre_resolution_logger(workspace: &Path) {
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"const fs = require('node:fs');
const path = require('node:path');
module.exports = {
  hooks: {
    preResolution(ctx) {
      const hasPackages = Boolean(ctx.currentLockfile?.packages && Object.keys(ctx.currentLockfile.packages).length > 0);
      fs.appendFileSync(path.join(__dirname, 'pre-res.jsonl'), `${JSON.stringify({ exists: ctx.existsCurrentLockfile, hasPackages })}\n`);
    },
  },
};
",
    )
    .expect("write .pnpmfile.cjs");
}

fn write_pkg_with_local_dep(workspace: &Path) {
    let dep_dir = workspace.join("deps/dep-a");
    fs::create_dir_all(&dep_dir).expect("create dep-a");
    fs::write(dep_dir.join("package.json"), r#"{"name":"dep-a","version":"1.0.0"}"#)
        .expect("write dep-a manifest");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::write(
        pkg_a.join("package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"dep-a":"file:../../deps/dep-a"}}"#,
    )
    .expect("write pkg-a manifest");
}

fn read_pre_res_entries(workspace: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(workspace.join("pre-res.jsonl"))
        .expect("read pre-res.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse jsonl line"))
        .collect()
}

#[test]
fn ci_dedicated_workspace_in_project_virtual_store_preserved_under_filter() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nenableGlobalVirtualStore: false\nvirtualStoreDir: .cache/pnpm\nstoreDir: {}\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    write_pre_resolution_logger(&workspace);
    write_pkg_with_local_dep(&workspace);

    let initial = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("install")
        .output()
        .expect("run initial install");
    assert!(initial.status.success(), "{initial:?}");
    let in_project_lock = workspace.join("packages/pkg-a/.cache/pnpm/lock.yaml");
    assert!(in_project_lock.is_file(), "in-project virtual store lockfile written");

    let _ = fs::remove_file(workspace.join("pre-res.jsonl"));
    let ci_out = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["--filter", "pkg-a", "ci"])
        .output()
        .expect("run filtered pnpm ci");
    assert!(ci_out.status.success(), "{ci_out:?}");
    assert!(in_project_lock.is_file(), "in-project virtual store lockfile preserved");
    let entries = read_pre_res_entries(&workspace);
    assert_eq!(entries, vec![serde_json::json!({ "exists": true, "hasPackages": true })]);

    drop(root);
}

#[test]
fn ci_dedicated_workspace_with_workspace_shared_modules_dir_cleans_on_unfiltered_only() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    let shared_nm = workspace.join("shared_nm");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nstoreDir: {}\nmodulesDir: {}\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
            serde_json::to_string(&shared_nm).expect("quote modulesDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    write_pre_resolution_logger(&workspace);
    write_pkg_with_local_dep(&workspace);

    let initial = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("install")
        .output()
        .expect("run initial install");
    assert!(initial.status.success(), "{initial:?}");
    assert!(shared_nm.join(".pnpm/lock.yaml").is_file(), "shared_nm lockfile written");

    fs::write(shared_nm.join("stale-marker"), "remove-me").expect("write stale marker");
    let _ = fs::remove_file(workspace.join("pre-res.jsonl"));
    let unfiltered = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("ci")
        .output()
        .expect("run unfiltered pnpm ci");
    assert!(unfiltered.status.success(), "{unfiltered:?}");
    assert!(!shared_nm.join("stale-marker").exists(), "root clean removed shared_nm contents");
    assert_eq!(
        read_pre_res_entries(&workspace),
        vec![serde_json::json!({ "exists": false, "hasPackages": false })],
    );

    fs::write(shared_nm.join("stale-marker"), "keep-me").expect("write stale marker");
    let _ = fs::remove_file(workspace.join("pre-res.jsonl"));
    let filtered = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["--filter", "pkg-a", "ci"])
        .output()
        .expect("run filtered pnpm ci");
    assert!(filtered.status.success(), "{filtered:?}");
    assert!(shared_nm.join("stale-marker").exists(), "filtered clean left shared_nm intact");
    assert_eq!(
        read_pre_res_entries(&workspace),
        vec![serde_json::json!({ "exists": true, "hasPackages": true })],
    );

    drop(root);
}

#[test]
fn ci_rootless_workspace_with_protected_modules_dir_preserves_current_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - apps/*\n  - packages/pkg-a\nmodulesDir: packages\nstoreDir: {}\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    write_pre_resolution_logger(&workspace);
    write_pkg_with_local_dep(&workspace);
    let app_a = workspace.join("apps/app-a");
    fs::create_dir_all(&app_a).expect("create app-a");
    fs::write(
        app_a.join("package.json"),
        r#"{"name":"app-a","version":"1.0.0","dependencies":{"dep-a":"file:../../deps/dep-a"}}"#,
    )
    .expect("write app-a manifest");

    let initial = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("install")
        .output()
        .expect("run initial install");
    assert!(initial.status.success(), "{initial:?}");
    assert!(
        workspace.join("packages/.pnpm/lock.yaml").is_file(),
        "packages/.pnpm/lock.yaml written",
    );

    let _ = fs::remove_file(workspace.join("pre-res.jsonl"));
    let ci_out = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("ci")
        .output()
        .expect("run pnpm ci");
    assert!(ci_out.status.success(), "{ci_out:?}");
    assert!(workspace.join("packages/pkg-a/package.json").is_file(), "member preserved");
    assert!(workspace.join("packages/.pnpm/lock.yaml").is_file(), "protected lock.yaml preserved");
    assert_eq!(
        read_pre_res_entries(&workspace),
        vec![serde_json::json!({ "exists": true, "hasPackages": true })],
    );

    drop(root);
}

#[test]
fn ci_shared_workspace_preserves_nonhidden_virtual_store_on_narrowed_runs() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - .\n  - packages/*\nenableGlobalVirtualStore: false\nvirtualStoreDir: node_modules/custom-store\nstoreDir: {}\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    write_pre_resolution_logger(&workspace);
    write_pkg_with_local_dep(&workspace);
    let dep_b = workspace.join("deps/dep-b");
    fs::create_dir_all(&dep_b).expect("create dep-b");
    fs::write(dep_b.join("package.json"), r#"{"name":"dep-b","version":"1.0.0"}"#)
        .expect("write dep-b manifest");
    fs::write(
        workspace.join("packages/pkg-a/package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"dep-b":"file:../../deps/dep-b"}}"#,
    )
    .expect("write pkg-a manifest");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"root-pkg","version":"1.0.0","dependencies":{"dep-a":"file:deps/dep-a"}}"#,
    )
    .expect("write root manifest");

    let initial = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("install")
        .output()
        .expect("run initial install");
    assert!(initial.status.success(), "{initial:?}");
    assert!(workspace.join("node_modules/custom-store/lock.yaml").is_file());

    let _ = fs::remove_file(workspace.join("pre-res.jsonl"));
    let member_ci = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["--filter", "pkg-a", "ci"])
        .output()
        .expect("run member-filtered pnpm ci");
    assert!(member_ci.status.success(), "{member_ci:?}");
    assert_eq!(
        read_pre_res_entries(&workspace),
        vec![serde_json::json!({ "exists": true, "hasPackages": true })],
    );

    fs::write(workspace.join("node_modules/custom-store/preserved-marker"), "keep-for-narrowed")
        .expect("write preserved marker");
    fs::create_dir_all(workspace.join("node_modules/stale-root-pkg")).expect("create stale pkg");
    let _ = fs::remove_file(workspace.join("pre-res.jsonl"));
    let root_ci = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["-w", "ci"])
        .output()
        .expect("run root-filtered pnpm ci");
    assert!(root_ci.status.success(), "{root_ci:?}");
    assert!(
        !workspace.join("node_modules/stale-root-pkg").exists(),
        "root direct entries were cleaned",
    );
    assert!(
        workspace.join("node_modules/custom-store/preserved-marker").is_file(),
        "shared custom-store inside root node_modules survives narrowed -w ci",
    );
    assert!(
        workspace.join("packages/pkg-a/node_modules/dep-b/package.json").is_file(),
        "unselected member dependency inside custom-store stays intact",
    );
    assert_eq!(
        read_pre_res_entries(&workspace),
        vec![serde_json::json!({ "exists": true, "hasPackages": true })],
    );

    let _ = fs::remove_file(workspace.join("pre-res.jsonl"));
    let full_ci = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("ci")
        .output()
        .expect("run unfiltered pnpm ci");
    assert!(full_ci.status.success(), "{full_ci:?}");
    assert!(
        !workspace.join("node_modules/custom-store/preserved-marker").exists(),
        "unfiltered pnpm ci cleans custom-store",
    );
    assert_eq!(
        read_pre_res_entries(&workspace),
        vec![serde_json::json!({ "exists": false, "hasPackages": false })],
    );

    drop(root);
}

#[test]
fn clean_rejects_virtual_store_and_root_modules_dir_inside_workspace_member() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pkg_b_src = workspace.join("packages/pkg-b/src");
    fs::create_dir_all(&pkg_b_src).expect("create pkg-b/src");
    fs::write(
        workspace.join("packages/pkg-b/package.json"),
        r#"{"name":"pkg-b","version":"1.0.0"}"#,
    )
    .expect("write pkg-b manifest");
    fs::write(pkg_b_src.join("index.js"), "export const answer = 42;\n")
        .expect("write pkg-b source");
    fs::write(workspace.join("package.json"), r#"{"name":"root-pkg","version":"1.0.0"}"#)
        .expect("write root manifest");

    for setting in ["virtualStoreDir: packages/pkg-b/src", "modulesDir: packages/pkg-b/src"] {
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!("packages:\n  - .\n  - packages/*\n{setting}\n"),
        )
        .expect("write pnpm-workspace.yaml");
        let output = Command::cargo_bin("pnpm")
            .expect("find pnpm")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_arg("clean")
            .output()
            .expect("run pnpm clean");
        assert!(output.status.success(), "{setting}: {output:?}");
        assert!(
            pkg_b_src.join("index.js").is_file(),
            "member source must survive clean under `{setting}`",
        );
    }

    drop(root);
}
