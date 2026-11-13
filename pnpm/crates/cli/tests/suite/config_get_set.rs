//! `pnpm get` / `pnpm set` — the top-level spellings of `pnpm config get`
//! and `pnpm config set`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::CommandTempCwd, command_env::CommandTestExt, diagnostics::assert_diagnostic_contains,
};
use pretty_assertions::assert_eq;
use std::{
    fs, io,
    process::{Command, Stdio},
};

fn pacquet_in(workspace: &std::path::Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_arg("--dir")
        .with_arg(workspace)
        .without_ambient_pnpm_config()
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

/// `pnpm config list | head -0` closes stdout before pnpm writes the
/// list; pnpm exits 0 instead of aborting (pnpm/tasks#132).
#[test]
fn config_list_exits_zero_when_stdout_is_closed() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let (stdout_reader, stdout_writer) = io::pipe().expect("create stdout pipe");
    drop(stdout_reader);

    let output = pacquet
        .with_args(["config", "list"])
        .with_stdout(Stdio::from(stdout_writer))
        .output()
        .expect("run pacquet config list");
    assert_eq!(output.status.code(), Some(0), "{output:?}");

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

/// A project's `pnpm-workspace.yaml` carries no machine-level state, so
/// `--location=project` refuses one and tells the user where it belongs.
#[test]
fn config_set_refuses_a_machine_level_key_in_the_project_manifest() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: ~/store\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args(["config", "set", "--location=project", "state-dir", "/somewhere"])
        .output()
        .expect("run pnpm config set");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "unexpected success: {stderr}");
    assert!(stderr.contains("ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING"), "stderr={stderr}");
    assert_diagnostic_contains(
        &stderr,
        "Set it for the machine instead: pnpm config set --global state-dir",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
            .expect("read pnpm-workspace.yaml"),
        "storeDir: ~/store\n",
    );

    drop(root);
}

/// `pnpm config get registries` prints a registry's `networkConcurrency` as
/// the `registries` entry in `pnpm-workspace.yaml` wrote it.
#[test]
fn config_get_registries_shows_a_registry_network_concurrency() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "registries:\n  https://npm.corp.example/:\n    scopes: ['@acme']\n    networkConcurrency: 4\n",
    )
    .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args(["config", "get", "registries", "--json"])
        .output()
        .expect("run pacquet config get registries");
    eprintln!("stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let registries: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("registries print as JSON");
    assert_eq!(registries["https://npm.corp.example/"]["networkConcurrency"], 4);

    drop(root);
}

/// `--global` and `--location=global` scope a read to the global config, so
/// the project's `pnpm-workspace.yaml` and `.npmrc` stay out of it.
/// <https://github.com/pnpm/pnpm/issues/16598>
#[test]
fn global_get_and_list_ignore_the_project_settings() {
    let CommandTempCwd {
        pacquet: _pacquet, root, workspace, ..
    } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "name": "repro", "version": "1.0.0" }"#)
        .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "minimumReleaseAge: 2880\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join(".npmrc"), "//project.test/:_authToken=project-token\n")
        .expect("write .npmrc");
    let config_home = root.path().join("xdg-config");
    fs::create_dir_all(config_home.join("pnpm")).expect("create global config directory");
    fs::write(config_home.join("pnpm/config.yaml"), "fetchRetries: 5\n")
        .expect("write global config");
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(&pnpm_home).expect("create pnpm home");
    let run = |args: &[&str]| {
        let output = pacquet_in(&workspace)
            .with_env("XDG_CONFIG_HOME", &config_home)
            .with_env("PNPM_HOME", &pnpm_home)
            .with_args(args)
            .output()
            .expect("run pacquet");
        eprintln!("{args:?} stderr={}", String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("stdout is UTF-8")
            .trim_end()
            .to_string()
    };

    assert_eq!(run(&["config", "get", "minimumReleaseAge"]), "2880");
    assert_eq!(run(&["config", "get", "minimumReleaseAge", "--global"]), "undefined");
    assert_eq!(run(&["config", "get", "minimumReleaseAge", "--location=global"]), "undefined");
    assert_eq!(run(&["get", "-g", "minimumReleaseAge"]), "undefined");
    assert_eq!(run(&["config", "get", "minimumReleaseAge", "-g", "--location=project"]), "2880");
    assert_eq!(run(&["config", "get", "//project.test/:_authToken", "--global"]), "undefined");
    assert_eq!(run(&["config", "get", "fetchRetries", "--global"]), "5");

    let list = run(&["config", "list", "--global"]);
    eprintln!("list={list}");
    assert!(!list.contains(r#"minimumReleaseAge""#));
    assert!(!list.contains("//project.test/"));
    assert!(list.contains(r#""fetchRetries": 5"#));

    let from_env_workspace = pacquet_in(root.path())
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PNPM_CONFIG_WORKSPACE_DIR", &workspace)
        .with_args(["config", "get", "minimumReleaseAge", "--global"])
        .output()
        .expect("run pacquet");
    eprintln!("stderr={}", String::from_utf8_lossy(&from_env_workspace.stderr));
    assert!(from_env_workspace.status.success());
    assert_eq!(String::from_utf8_lossy(&from_env_workspace.stdout).trim_end(), "undefined");

    drop(root);
}

#[test]
fn flow_mapping_workspace_yaml_rejects_registry_env_and_drops_proxy_env() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name": "demo-app", "version": "1.0.0"}"#)
        .expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "{registry: https://example.invalid/${DEMO_REG_VAR}/}\n",
    )
    .expect("write flow pnpm-workspace.yaml");

    let registry_out = pacquet_in(&workspace)
        .with_env("DEMO_REG_VAR", "subpath")
        .with_args(["config", "get", "registry"])
        .output()
        .expect("run pacquet config get registry");
    let stderr = String::from_utf8_lossy(&registry_out.stderr);
    let stdout = String::from_utf8_lossy(&registry_out.stdout);
    assert!(!registry_out.status.success(), "expected failure, stdout={stdout}, stderr={stderr}");
    assert!(stderr.contains("ERR_PNPM_REGISTRY_ENV_NOT_EXPANDED"), "stderr={stderr}");
    assert!(!stdout.contains("subpath") && !stderr.contains("subpath"), "leaked env value");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "{httpsProxy: http://127.0.0.1:8080/${DEMO_REG_VAR}, shamefullyHoist: ${DEMO_HOIST}}\n",
    )
    .expect("write flow proxy pnpm-workspace.yaml");
    let proxy_get = pacquet_in(&workspace)
        .with_env("DEMO_REG_VAR", "subpath")
        .with_env("DEMO_HOIST", "true")
        .with_args(["config", "get", "httpsProxy"])
        .output()
        .expect("run pacquet config get httpsProxy");
    assert!(proxy_get.status.success(), "stderr={}", String::from_utf8_lossy(&proxy_get.stderr));
    assert_eq!(String::from_utf8_lossy(&proxy_get.stdout).trim_end(), "undefined");

    let proxy_list = pacquet_in(&workspace)
        .with_env("DEMO_REG_VAR", "subpath")
        .with_env("DEMO_HOIST", "true")
        .with_args(["config", "list"])
        .output()
        .expect("run pacquet config list");
    let list_stderr = String::from_utf8_lossy(&proxy_list.stderr);
    let list_stdout = String::from_utf8_lossy(&proxy_list.stdout);
    assert!(proxy_list.status.success(), "stderr={list_stderr}");
    assert!(list_stderr.contains(r#"Ignored "httpsProxy""#), "stderr={list_stderr}");
    assert!(!list_stdout.contains("subpath") && !list_stderr.contains("subpath"));
    assert!(list_stdout.contains(r#""shamefullyHoist": true"#), "stdout={list_stdout}");

    let config_home = root.path().join("xdg-config");
    fs::create_dir_all(config_home.join("pnpm")).expect("create global config directory");
    fs::write(
        config_home.join("pnpm/config.yaml"),
        "{registry: https://example.invalid/${DEMO_REG_VAR}/}\n",
    )
    .expect("write global config.yaml");
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(&pnpm_home).expect("create pnpm home");
    let global_out = pacquet_in(&workspace)
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("DEMO_REG_VAR", "subpath")
        .with_args(["config", "get", "registry", "--global"])
        .output()
        .expect("run global config get registry");
    assert!(global_out.status.success(), "stderr={}", String::from_utf8_lossy(&global_out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&global_out.stdout).trim_end(),
        "https://example.invalid/subpath/",
    );

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "{include: {'./shared.yaml': [shamefullyHoist]}, userAgent: agent/${DEMO_UNSET_VAR}}\n",
    )
    .expect("write root include flow pnpm-workspace.yaml");
    fs::write(workspace.join("shared.yaml"), "shamefullyHoist: true\n").expect("write shared.yaml");
    let include_get = pacquet_in(&workspace)
        .with_args(["config", "get", "shamefully-hoist"])
        .output()
        .expect("run config get shamefully-hoist with include");
    assert!(
        include_get.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&include_get.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&include_get.stdout).trim_end(), "true");

    let include_list = pacquet_in(&workspace)
        .with_args(["config", "list"])
        .output()
        .expect("run config list with include");
    let include_list_stderr = String::from_utf8_lossy(&include_list.stderr);
    let include_list_stdout = String::from_utf8_lossy(&include_list.stdout);
    assert!(include_list.status.success(), "stderr={include_list_stderr}");
    assert!(include_list_stderr.contains(r#"Ignored "userAgent""#), "stderr={include_list_stderr}");
    assert!(
        include_list_stdout.contains(r#""shamefullyHoist": true"#),
        "stdout={include_list_stdout}",
    );

    fs::write(workspace.join("pnpm-workspace.yaml"), "include:\n  ./shared.yaml: [registry]\n")
        .expect("write include registry pnpm-workspace.yaml");
    fs::write(
        workspace.join("shared.yaml"),
        "{registry: https://example.invalid/${DEMO_REG_VAR}/}\n",
    )
    .expect("write flow registry shared.yaml");
    let include_reg = pacquet_in(&workspace)
        .with_env("DEMO_REG_VAR", "subpath")
        .with_args(["config", "get", "registry"])
        .output()
        .expect("run config get registry with included flow placeholder");
    let include_reg_stderr = String::from_utf8_lossy(&include_reg.stderr);
    let include_reg_stdout = String::from_utf8_lossy(&include_reg.stdout);
    assert!(
        !include_reg.status.success(),
        "stdout={include_reg_stdout}, stderr={include_reg_stderr}",
    );
    assert!(
        include_reg_stderr.contains("ERR_PNPM_REGISTRY_ENV_NOT_EXPANDED"),
        "stderr={include_reg_stderr}",
    );
    assert!(!include_reg_stdout.contains("subpath") && !include_reg_stderr.contains("subpath"));

    drop(root);
}
