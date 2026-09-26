//! `pnpm get` / `pnpm set` — the top-level spellings of `pnpm config get`
//! and `pnpm config set`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::{fs, process::Command};

fn pacquet_in(workspace: &std::path::Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

#[test]
fn set_writes_the_setting_and_get_reads_it_back() {
    let CommandTempCwd {
        pacquet: _pacquet, root, workspace, ..
    } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");

    let set = pacquet_in(&workspace)
        .with_args(["set", "node-linker", "hoisted", "--location", "project"])
        .output()
        .expect("run pacquet set");
    eprintln!("set stderr={}", String::from_utf8_lossy(&set.stderr));
    assert!(set.status.success());

    let get = pacquet_in(&workspace)
        .with_args(["get", "node-linker"])
        .output()
        .expect("run pacquet get");
    eprintln!("get stderr={}", String::from_utf8_lossy(&get.stderr));
    assert!(get.status.success());
    assert_eq!(String::from_utf8_lossy(&get.stdout).trim_end(), "hoisted");

    drop(root);
}

/// Run from a workspace sub-package, `--location=project` writes the
/// workspace root's `pnpm-workspace.yaml`. A manifest created in the
/// sub-package would make it the workspace root.
/// <https://github.com/pnpm/pnpm/issues/13757>
#[test]
fn set_with_location_project_from_a_sub_package_writes_the_workspace_root() {
    let CommandTempCwd {
        pacquet: _pacquet, root, workspace, ..
    } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    let sub_package = workspace.join("packages/a");
    fs::create_dir_all(&sub_package).expect("create sub-package");
    fs::write(sub_package.join("package.json"), r#"{ "name": "a", "version": "1.0.0" }"#)
        .expect("write package.json");

    let set = pacquet_in(&sub_package)
        .with_args(["config", "set", "--location=project", "node-linker", "hoisted"])
        .output()
        .expect("run pacquet config set");
    eprintln!("set stderr={}", String::from_utf8_lossy(&set.stderr));
    assert!(set.status.success());

    assert!(!sub_package.join("pnpm-workspace.yaml").exists());
    let root_manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read root manifest");
    assert!(root_manifest.contains("nodeLinker: hoisted"), "root manifest: {root_manifest}");

    drop(root);
}

/// `pnpm get <key>` prints one value for a script to capture, so the
/// report has to be the only thing on stdout.
#[test]
fn get_keeps_stdout_to_the_value() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args(["get", "node-linker"])
        .output()
        .expect("run pacquet get");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim_end(), "hoisted");

    drop(root);
}

#[test]
fn config_set_can_bootstrap_auth_for_a_global_custom_registry_before_switching_versions() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "packageManager": "pnpm@0.0.0" }"#)
        .expect("write package.json");
    let config_home = root.path().join("xdg-config");
    let config_dir = config_home.join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config directory");
    fs::write(config_dir.join("auth.ini"), "registry=http://127.0.0.1:1/\n")
        .expect("write global config");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("PNPM_CONFIG_FETCH_RETRIES", "0")
        .with_args(["config", "set", "//127.0.0.1:1/:_auth", "secret"])
        .output()
        .expect("run pacquet config set");
    eprintln!("stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert!(
        fs::read_to_string(config_dir.join("auth.ini"))
            .expect("read global auth config")
            .contains("//127.0.0.1:1/:_auth=secret"),
    );

    drop(root);
}

#[test]
fn config_set_checks_the_package_manager_when_writing_project_configuration() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "packageManager": "yarn@4.0.0" }"#)
        .expect("write package.json");

    let output = pacquet
        .with_args(["config", "set", "--location=project", "node-linker", "hoisted"])
        .output()
        .expect("run pacquet config set");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(!output.status.success(), "unexpected success: {stderr}");
    assert!(stderr.contains("This project is configured to use yarn"), "stderr={stderr}");

    drop(root);
}

#[test]
fn config_set_preserves_repeated_ca_and_comments_in_npmrc() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let npmrc_path = workspace.join(".npmrc");
    fs::write(
        &npmrc_path,
        "# Corporate CA certificates\nca=certificate-A\nca=certificate-B\n\n; Registry config\nregistry=https://registry.npmjs.org/\n",
    )
    .expect("write .npmrc");

    let output = pacquet_in(&workspace)
        .with_args([
            "config",
            "set",
            "--location=project",
            "registry",
            "https://registry.example.com/",
        ])
        .output()
        .expect("run pacquet config set");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr={stderr}");

    let text = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    assert!(text.contains("# Corporate CA certificates"));
    assert!(text.contains("; Registry config"));
    assert!(text.contains("ca=certificate-A"));
    assert!(text.contains("ca=certificate-B"));
    assert!(text.contains("registry=https://registry.example.com/"));
    assert!(!text.contains("registry=https://registry.npmjs.org/"));

    drop(root);
}

#[test]
fn config_delete_preserves_repeated_ca_and_comments_in_npmrc() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let npmrc_path = workspace.join(".npmrc");
    fs::write(
        &npmrc_path,
        "# Corporate CA certificates\nca=certificate-A\nca=certificate-B\n\n; Registry config\nregistry=https://registry.npmjs.org/\n",
    )
    .expect("write .npmrc");

    let output = pacquet_in(&workspace)
        .with_args(["config", "delete", "--location=project", "registry"])
        .output()
        .expect("run pacquet config delete");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr={stderr}");

    let text = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    assert!(text.contains("# Corporate CA certificates"));
    assert!(text.contains("; Registry config"));
    assert!(text.contains("ca=certificate-A"));
    assert!(text.contains("ca=certificate-B"));
    assert!(!text.contains("registry="));

    drop(root);
}

#[test]
fn config_set_ca_array_json_writes_unbracketed_ca_keys_to_clean_file() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let npmrc_path = workspace.join(".npmrc");
    fs::write(&npmrc_path, "# Existing comment\nregistry=https://registry.npmjs.org/\n")
        .expect("write .npmrc");

    let output = pacquet_in(&workspace)
        .with_args([
            "config",
            "set",
            "--json",
            "--location=project",
            "ca",
            r#"["cert-x", "cert-y"]"#,
        ])
        .output()
        .expect("run pacquet config set");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr={stderr}");

    let text = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    assert!(text.contains("# Existing comment"));
    assert!(text.contains("ca=cert-x"));
    assert!(text.contains("ca=cert-y"));
    assert!(!text.contains("ca[]="));
    assert!(text.contains("registry=https://registry.npmjs.org/"));

    drop(root);
}
