//! `pnpm env`: the deprecated Node.js front end to `pnpm runtime`.
//!
//! `env use`, `env list`, and `env remove` exercise the runtime aliases
//! against the mocked Node.js mirror and global install directory.

use command_extra::CommandExtra;
use pnpm_testing_utils_registry::bin::CommandTempCwd;

struct EnvOutput {
    succeeded: bool,
    /// `env` is not one of pnpm's stderr-reporter commands, so its reporter
    /// output lands here while the miette error goes to stderr.
    stdout: String,
    stderr: String,
}

fn run_env(args: &[&str]) -> EnvOutput {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let output = pacquet
        .with_env("PNPM_HOME", root.path().join("pnpm_home"))
        .with_args(args)
        .output()
        .unwrap_or_else(|error| panic!("run pacquet env {args:?}: {error}"));
    let outcome = EnvOutput {
        succeeded: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    eprintln!("{args:?} stdout={} stderr={}", outcome.stdout, outcome.stderr);
    drop(root);
    outcome
}

fn prepend_to_path(dir: &std::path::Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).expect("join PATH")
}

#[test]
fn a_bare_env_asks_for_a_subcommand() {
    let output = run_env(&["env"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_RUNTIME_NO_SUBCOMMAND"), "{}", output.stderr);
}

#[test]
fn an_unknown_subcommand_is_rejected() {
    let output = run_env(&["env", "install"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_RUNTIME_UNKNOWN_SUBCOMMAND"), "{}", output.stderr);
}

#[test]
fn use_refuses_to_manage_node_when_the_global_bin_is_not_on_path() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm_home");
    std::fs::create_dir_all(&pnpm_home).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["env", "use", "--global", "24"])
        .output()
        .expect("run pacquet env use");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr={stderr}");
    assert!(stderr.contains("ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn env_list_use_and_remove_support_leading_v_and_prerelease_channels() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut server = mockito::Server::new();
    let version = "22.0.0-rc.1";
    let _mocks = crate::install_runtimes::mock_node_release(&mut server, version);

    let pnpm_home = root.path().join("pnpm_home");
    let global_bin = pnpm_home.join("bin");
    std::fs::create_dir_all(&global_bin).unwrap();
    std::fs::write(
        pnpm_home.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nnodeDownloadMirrors:\n  rc: '{}/'\n",
            root.path().join("store").display(),
            root.path().join("cache").display(),
            server.url(),
        ),
    )
    .unwrap();
    let _ = &npmrc_info;

    let config_home = root.path().join("xdg-config");
    let run = |args: &[&str]| {
        crate::_utils::pacquet_in(&workspace)
            .with_env("PNPM_HOME", &pnpm_home)
            .with_env("XDG_CONFIG_HOME", &config_home)
            .with_env("PATH", prepend_to_path(&global_bin))
            .with_args(args)
            .output()
            .expect("run pacquet env")
    };

    let list_out = run(&["env", "list", "--remote", "v22.0.0-rc.1"]);
    assert!(
        list_out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&list_out.stdout),
        String::from_utf8_lossy(&list_out.stderr),
    );
    assert!(
        String::from_utf8_lossy(&list_out.stdout).contains("22.0.0-rc.1"),
        "stdout={}",
        String::from_utf8_lossy(&list_out.stdout),
    );

    let use_out = run(&["env", "use", "-g", "v22.0.0-rc.1"]);
    assert!(
        use_out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&use_out.stdout),
        String::from_utf8_lossy(&use_out.stderr),
    );

    let remove_out = run(&["env", "remove", "-g", "v22.0.0-rc.1"]);
    assert!(
        remove_out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&remove_out.stdout),
        String::from_utf8_lossy(&remove_out.stderr),
    );
    assert!(
        pnpm_global::find_global_package(&pnpm_home.join("global/v11"), "node").unwrap().is_none(),
        "node global package should be removed",
    );

    let reinstall_out = run(&["env", "use", "-g", "v22.0.0-rc.1"]);
    assert!(reinstall_out.status.success());
    assert!(
        pnpm_global::find_global_package(&pnpm_home.join("global/v11"), "node").unwrap().is_some(),
        "node global package should be reinstalled",
    );
    let remove_rc = run(&["env", "remove", "-g", "rc/22.0.0-rc.1"]);
    assert!(remove_rc.status.success(), "stderr={}", String::from_utf8_lossy(&remove_rc.stderr));
    assert!(
        pnpm_global::find_global_package(&pnpm_home.join("global/v11"), "node").unwrap().is_none(),
        "node global package should be removed by rc/22.0.0-rc.1",
    );

    let legacy_dir = pnpm_home.join("nodejs/22.14.0");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let remove_stable_v = run(&["env", "remove", "-g", "v22.14.0"]);
    assert!(
        remove_stable_v.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&remove_stable_v.stderr),
    );
    assert!(!legacy_dir.exists());
}
