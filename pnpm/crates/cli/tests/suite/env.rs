//! `pnpm env` — the deprecated Node.js front end to `pnpm runtime`.
//!
//! Only the paths that stop before the network are covered here; `env use`
//! and `env list` reach the Node.js mirror, which the mocked registry does
//! not serve.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;

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
    assert!(output.stderr.contains("ERR_PNPM_ENV_NO_SUBCOMMAND"), "{}", output.stderr);
}

#[test]
fn an_unknown_subcommand_is_rejected() {
    let output = run_env(&["env", "install"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_ENV_UNKNOWN_SUBCOMMAND"), "{}", output.stderr);
}

/// pnpm warns before it validates, so the deprecation notice reaches a user
/// whose invocation is about to be rejected for another reason.
#[test]
fn use_without_global_is_refused_after_the_deprecation_warning() {
    let output = run_env(&["env", "use", "24"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_NOT_IMPLEMENTED_YET"), "{}", output.stderr);
    assert!(output.stdout.contains(r#""pnpm env use" is deprecated"#), "{}", output.stdout);
}

#[test]
fn remove_without_global_is_refused_after_the_deprecation_warning() {
    let output = run_env(&["env", "remove", "24"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_NOT_IMPLEMENTED_YET"), "{}", output.stderr);
    assert!(output.stdout.contains(r#""pnpm env remove" is deprecated"#), "{}", output.stdout);
}

#[test]
fn rm_without_global_is_refused_after_the_deprecation_warning() {
    let output = run_env(&["env", "rm", "24"]);
    assert!(!output.succeeded);
    assert!(output.stderr.contains("ERR_PNPM_NOT_IMPLEMENTED_YET"), "{}", output.stderr);
    assert!(output.stdout.contains(r#""pnpm env remove" is deprecated"#), "{}", output.stdout);
}

#[cfg(unix)]
#[test]
fn remove_cleans_up_dangling_node_link() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm_home");
    let global_bin = pnpm_home.join("bin");
    std::fs::create_dir_all(&global_bin).unwrap();

    let dangling_target = pnpm_home.join("nodejs/18.12.1/bin/node");
    let bin_node = global_bin.join("node");
    std::os::unix::fs::symlink(&dangling_target, &bin_node).unwrap();
    assert!(bin_node.is_symlink());
    assert!(!bin_node.exists());

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", prepend_to_path(&global_bin))
        .with_args(["--global", "env", "rm", "18.12"])
        .output()
        .expect("run pacquet env rm");

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(!bin_node.is_symlink());
    assert!(!bin_node.exists());
}

#[cfg(unix)]
#[test]
fn remove_multiple_versions() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm_home");
    let global_bin = pnpm_home.join("bin");
    let nodejs_dir = pnpm_home.join("nodejs");
    std::fs::create_dir_all(&global_bin).unwrap();
    std::fs::create_dir_all(nodejs_dir.join("14.0.0")).unwrap();
    std::fs::create_dir_all(nodejs_dir.join("16.2.3")).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", prepend_to_path(&global_bin))
        .with_args(["--global", "env", "rm", "14.0.0", "16.2.3"])
        .output()
        .expect("run pacquet env rm");

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(!nodejs_dir.join("14.0.0").exists());
    assert!(!nodejs_dir.join("16.2.3").exists());
}

#[cfg(unix)]
#[test]
fn remove_does_not_delete_prefix_unrelated_versions() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm_home");
    let global_bin = pnpm_home.join("bin");
    let nodejs_dir = pnpm_home.join("nodejs");
    std::fs::create_dir_all(&global_bin).unwrap();
    std::fs::create_dir_all(nodejs_dir.join("20.8.0")).unwrap();
    std::fs::create_dir_all(nodejs_dir.join("22.0.0")).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", prepend_to_path(&global_bin))
        .with_args(["--global", "env", "rm", "2"])
        .output()
        .expect("run pacquet env rm");

    assert!(!output.status.success());
    assert!(nodejs_dir.join("20.8.0").exists());
    assert!(nodejs_dir.join("22.0.0").exists());
}

#[test]
fn remove_cleans_up_windows_cmd_shims_without_symlink() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm_home");
    let global_bin = pnpm_home.join("bin");
    let nodejs_dir = pnpm_home.join("nodejs");
    std::fs::create_dir_all(&global_bin).unwrap();
    std::fs::create_dir_all(nodejs_dir.join("18.12.0")).unwrap();

    let cmd_shim = global_bin.join("node.cmd");
    let ps1_shim = global_bin.join("node.ps1");
    std::fs::write(&cmd_shim, r#"@"%~dp0\..\nodejs\18.12.0\node.exe" %*"#).unwrap();
    std::fs::write(&ps1_shim, r#"& "$PSScriptRoot\..\nodejs\18.12.0\node.exe" @args"#).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", prepend_to_path(&global_bin))
        .with_args(["--global", "env", "rm", "18.12.0"])
        .output()
        .expect("run pacquet env rm");

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(!cmd_shim.exists());
    assert!(!ps1_shim.exists());
}

#[test]
fn remove_preserves_windows_cmd_shims_targeting_longer_version() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm_home");
    let global_bin = pnpm_home.join("bin");
    let nodejs_dir = pnpm_home.join("nodejs");
    std::fs::create_dir_all(&global_bin).unwrap();
    std::fs::create_dir_all(nodejs_dir.join("18.1.0")).unwrap();
    std::fs::create_dir_all(nodejs_dir.join("18.10.0")).unwrap();

    let cmd_shim = global_bin.join("node.cmd");
    std::fs::write(&cmd_shim, r#"@"%~dp0\..\nodejs\18.10.0\node.exe" %*"#).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", prepend_to_path(&global_bin))
        .with_args(["--global", "env", "rm", "18.1.0"])
        .output()
        .expect("run pacquet env rm");

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(cmd_shim.exists());
    assert!(!nodejs_dir.join("18.1.0").exists());
    assert!(nodejs_dir.join("18.10.0").exists());
}

#[test]
fn remove_with_custom_global_dir_removes_configured_global_node() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm_home");
    let global_bin = pnpm_home.join("bin");
    let custom_global_dir = root.path().join("custom_global");
    let custom_pkg_dir = custom_global_dir.join("v11");
    let install_dir = custom_pkg_dir.join("install-node");
    let node_pkg_dir = install_dir.join("node_modules").join("node");

    std::fs::create_dir_all(&global_bin).unwrap();
    std::fs::create_dir_all(&node_pkg_dir).unwrap();
    std::fs::write(
        install_dir.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "engines": {
                "runtime": { "name": "node", "version": "18.12.0", "onFail": "download" },
            },
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        node_pkg_dir.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": "node",
            "version": "18.12.0",
            "bin": { "node": "bin/node" }
        }))
        .unwrap(),
    )
    .unwrap();
    pnpm_fs::symlink_dir(&install_dir, &custom_pkg_dir.join("hash-node")).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", prepend_to_path(&global_bin))
        .with_args([
            "--global",
            &format!("--global-dir={}", custom_global_dir.display()),
            "env",
            "rm",
            "18.12.0",
        ])
        .output()
        .expect("run pacquet env rm");

    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(!custom_pkg_dir.join("hash-node").exists());
}
