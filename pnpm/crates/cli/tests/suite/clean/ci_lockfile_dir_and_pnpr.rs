use super::{CommandTempCwd, seed_package};
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, path::Path, process::Command};

fn write_local_dep(workspace: &Path, name: &str) {
    let dep_dir = workspace.join("deps").join(name);
    fs::create_dir_all(&dep_dir).expect("create local dep dir");
    fs::write(
        dep_dir.join("package.json"),
        serde_json::json!({ "name": name, "version": "1.0.0" }).to_string(),
    )
    .expect("write local dep manifest");
}

#[test]
fn ci_preserves_node_modules_when_pnpr_server_conflicts_with_frozen_store() {
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

    let install = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("install")
        .output()
        .expect("run install");
    assert!(install.status.success(), "{install:?}");

    let marker = workspace.join("node_modules/keep-marker.txt");
    fs::write(&marker, "keep").expect("write marker");

    let ci_out = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["ci", "--pnpr-server", "https://pnpr.example.com", "--frozen-store"])
        .output()
        .expect("run ci");
    assert!(!ci_out.status.success(), "ci must fail on pnpr + frozenStore");
    let stderr = String::from_utf8_lossy(&ci_out.stderr);
    assert!(
        stderr.contains("ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR"),
        "expected ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR, got: {stderr}",
    );
    assert!(marker.exists(), "node_modules marker must survive pnpr config conflict");
    assert!(workspace.join("node_modules/dep-a/package.json").exists());

    drop(root);
}

#[test]
fn ci_cli_lockfile_dir_overrides_workspace_yaml_and_enforces_symlink_containment() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_local_dep(&workspace, "dep-a");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nlockfileDir: yaml-lock\nstoreDir: {}\n",
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

    // Seed a valid lockfile in cli-lock inside the workspace and verify `ci --lockfile-dir`
    // cleans cli-lock/node_modules rather than yaml-lock/node_modules.
    let cli_lock = workspace.join("cli-lock");
    fs::create_dir_all(&cli_lock).expect("create cli-lock");
    let yaml_lock = workspace.join("yaml-lock");
    seed_package(&yaml_lock.join("node_modules"), "keep-yaml");
    let gen_lock = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["install", "--lockfile-only", "--lockfile-dir", "cli-lock"])
        .output()
        .expect("generate lockfile");
    assert!(gen_lock.status.success(), "{gen_lock:?}");
    seed_package(&cli_lock.join("node_modules"), "stale-cli");

    let ci_cli = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["ci", "--lockfile-dir", "cli-lock"])
        .output()
        .expect("run ci --lockfile-dir cli-lock");
    assert!(ci_cli.status.success(), "{ci_cli:?}");
    assert!(
        !cli_lock.join("node_modules/stale-cli").exists(),
        "cli-lock node_modules must be cleaned",
    );
    assert!(
        yaml_lock.join("node_modules/keep-yaml").exists(),
        "overridden yaml-lock node_modules must not be cleaned",
    );

    // Symlink-parent escape via `--lockfile-dir escape/../keep-dir`: `keep-dir` inside workspace
    // must NOT be cleaned when `escape` points outside the workspace.
    let outside = root.path().join("outside");
    fs::create_dir_all(outside.join("sub")).expect("create outside/sub");
    fs::create_dir_all(outside.join("keep-dir")).expect("create outside/keep-dir");
    fs::copy(cli_lock.join("pnpm-lock.yaml"), workspace.join("keep-dir-src.yaml"))
        .expect("copy lockfile");
    let ws_keep = workspace.join("keep-dir");
    fs::create_dir_all(&ws_keep).expect("create ws keep-dir");
    fs::rename(workspace.join("keep-dir-src.yaml"), ws_keep.join("pnpm-lock.yaml"))
        .expect("place lockfile");
    fs::copy(ws_keep.join("pnpm-lock.yaml"), outside.join("keep-dir/pnpm-lock.yaml"))
        .expect("place outside lockfile");
    seed_package(&ws_keep.join("node_modules"), "keep-inside");
    pnpm_fs::symlink_dir(&outside.join("sub"), &workspace.join("escape"))
        .expect("symlink escape -> outside/sub");

    let _ = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["ci", "--lockfile-dir", "escape/../keep-dir"])
        .output()
        .expect("run ci with symlink-parent lockfile-dir");
    assert!(
        ws_keep.join("node_modules/keep-inside").exists(),
        "symlink-parent traversal via --lockfile-dir must not clean workspace/keep-dir/node_modules",
    );

    drop(root);
}

#[test]
fn clean_lockfile_resolves_env_lockfile_dir_relative_to_cli_dir() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nlockfileDir: root-lock\n",
    )
    .expect("write pnpm-workspace.yaml");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a"}"#).expect("write pkg-a manifest");

    let root_lock = workspace.join("root-lock");
    fs::create_dir_all(&root_lock).expect("create root-lock");
    fs::write(root_lock.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("write root-lock lockfile");

    let sub_lock = workspace.join("packages/sub-lock");
    fs::create_dir_all(&sub_lock).expect("create packages/sub-lock");
    fs::write(sub_lock.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("write sub-lock lockfile");

    // Empty env var must NOT mask `lockfileDir: root-lock` from pnpm-workspace.yaml.
    let empty_env = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&pkg_a)
        .without_ambient_pnpm_config()
        .with_env("PNPM_CONFIG_LOCKFILE_DIR", "")
        .with_args(["clean", "--lockfile"])
        .output()
        .expect("run clean with empty PNPM_CONFIG_LOCKFILE_DIR");
    assert!(empty_env.status.success(), "{empty_env:?}");
    assert!(
        !root_lock.join("pnpm-lock.yaml").exists(),
        "empty PNPM_CONFIG_LOCKFILE_DIR must fall through to pnpm-workspace.yaml lockfileDir",
    );
    assert!(sub_lock.join("pnpm-lock.yaml").exists());

    // Non-empty env var from subdirectory `packages/pkg-a` resolves relative to `packages/pkg-a`.
    fs::write(root_lock.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("restore root-lock lockfile");
    let non_empty_env = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&pkg_a)
        .without_ambient_pnpm_config()
        .with_env("PNPM_CONFIG_LOCKFILE_DIR", "../sub-lock")
        .with_args(["clean", "--lockfile"])
        .output()
        .expect("run clean with relative PNPM_CONFIG_LOCKFILE_DIR");
    assert!(non_empty_env.status.success(), "{non_empty_env:?}");
    assert!(
        !sub_lock.join("pnpm-lock.yaml").exists(),
        "PNPM_CONFIG_LOCKFILE_DIR from subdir must resolve relative to cli_dir",
    );
    assert!(
        root_lock.join("pnpm-lock.yaml").exists(),
        "overridden root-lock lockfile must remain untouched",
    );

    drop(root);
}

#[test]
fn clean_lockfile_enforces_symlink_containment_after_env_var_substitution_and_hook_override() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"app"}"#).expect("write package.json");
    let outside = root.path().join("outside");
    fs::create_dir_all(outside.join("sub")).expect("create outside/sub");
    pnpm_fs::symlink_dir(&outside.join("sub"), &workspace.join("escape"))
        .expect("symlink escape -> outside/sub");
    let keep_dir = workspace.join("keep-dir");
    fs::create_dir_all(&keep_dir).expect("create keep-dir");
    fs::write(keep_dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("write keep-dir lockfile");

    let env_subst = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_env("ESCAPE_DIR", "escape/../keep-dir")
        .with_env("PNPM_CONFIG_LOCKFILE_DIR", "${ESCAPE_DIR}")
        .with_args(["clean", "--lockfile"])
        .output()
        .expect("run clean with substituted PNPM_CONFIG_LOCKFILE_DIR");
    assert!(env_subst.status.success(), "{env_subst:?}");
    assert!(
        keep_dir.join("pnpm-lock.yaml").exists(),
        "substituted PNPM_CONFIG_LOCKFILE_DIR must not bypass symlink-parent containment",
    );

    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig(cfg) { cfg.lockfileDir = 'escape/../keep-dir'; return cfg } } }\n",
    )
    .expect("write .pnpmfile.cjs");
    let hook_override = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_env("PNPM_CONFIG_LOCKFILE_DIR", "env-lock")
        .with_args(["clean", "--lockfile"])
        .output()
        .expect("run clean with updateConfig overriding PNPM_CONFIG_LOCKFILE_DIR");
    assert!(hook_override.status.success(), "{hook_override:?}");
    assert!(
        keep_dir.join("pnpm-lock.yaml").exists(),
        "updateConfig overriding PNPM_CONFIG_LOCKFILE_DIR must not bypass symlink-parent containment",
    );

    drop(root);
}

#[test]
fn clean_rootless_globstar_workspace_removes_custom_modules_dir_with_broken_manifests() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - '**'\nmodulesDir: custom_nm\n",
    )
    .expect("write pnpm-workspace.yaml");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#)
        .expect("write pkg-a manifest");
    seed_package(&pkg_a.join("custom_nm"), "member-dep");
    fs::create_dir_all(pkg_a.join("custom_nm/broken")).expect("create broken member dir");
    fs::write(pkg_a.join("custom_nm/broken/package.json"), "{ invalid json")
        .expect("write malformed member installed manifest");

    let root_custom_nm = workspace.join("custom_nm");
    seed_package(&root_custom_nm, "lodash");
    fs::create_dir_all(root_custom_nm.join("broken")).expect("create broken installed dir");
    fs::write(root_custom_nm.join("broken/package.json"), "{ invalid json")
        .expect("write malformed installed manifest");

    let out = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("clean")
        .output()
        .expect("run clean");
    assert!(out.status.success(), "{out:?}");
    assert!(!root_custom_nm.join("lodash").exists(), "root custom_nm/lodash removed");
    assert!(!root_custom_nm.join("broken").exists(), "root custom_nm/broken removed");
    assert!(!pkg_a.join("custom_nm/member-dep").exists(), "member custom_nm removed");
    assert!(pkg_a.join("package.json").exists(), "member manifest preserved");

    drop(root);
}

#[test]
fn clean_with_root_manifest_preserves_members_under_overlapping_modules_dir() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nmodulesDir: packages\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{"name":"root-pkg","version":"1.0.0"}"#)
        .expect("write root manifest");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(pkg_a.join("src")).expect("create pkg-a/src");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#)
        .expect("write pkg-a manifest");
    fs::write(pkg_a.join("src/index.js"), "module.exports = 1;\n").expect("write pkg-a source");

    for args in [&["clean", "--filter", "pkg-a"][..], &["clean"][..]] {
        seed_package(&pkg_a.join("packages"), "seeded-dep");
        let out = Command::cargo_bin("pnpm")
            .expect("find pnpm")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_args(args)
            .output()
            .expect("run clean");
        assert!(out.status.success(), "{args:?} failed: {out:?}");
        assert!(!pkg_a.join("packages/seeded-dep").exists(), "{args:?} removed member modulesDir");
        assert!(pkg_a.join("package.json").exists(), "{args:?} preserved member manifest");
        assert!(pkg_a.join("src/index.js").exists(), "{args:?} preserved member source");
    }

    drop(root);
}
