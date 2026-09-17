/// The built-in commands documented to prefer a same-named `package.json`
/// script — `deploy`, `rebuild`, and `setup` here; `clean` has its own
/// coverage in `clean.rs`
/// (<https://pnpm.io/scripts#built-in-command-and-script-name-conflicts>).
/// `pnpm pm <name>` forces the built-in command. The tests drive `/bin/sh`
/// scripts, so they are Unix-only.
#[cfg(unix)]
mod scripts {
    use assert_cmd::prelude::CommandCargoExt;
    use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
    use std::{fs, path::Path, process::Command};

    fn write_manifest(dir: &Path, name: &str, scripts: &serde_json::Value) {
        let manifest = serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "scripts": scripts,
        });
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }

    /// A workspace whose root declares no scripts and whose `packages/a`
    /// member declares the given ones.
    fn workspace_with_member_scripts(workspace: &Path, scripts: &serde_json::Value) {
        write_manifest(workspace, "root-pkg", &serde_json::json!({}));
        fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
            .expect("write pnpm-workspace.yaml");
        let member = workspace.join("packages").join("a");
        fs::create_dir_all(&member).expect("create the workspace member");
        write_manifest(&member, "a", scripts);
    }

    fn run(workspace: &Path, args: &[&str]) -> std::process::Output {
        let mut command =
            Command::cargo_bin("pnpm").expect("find the pnpm binary").without_ambient_pnpm_config();
        command.current_dir(workspace).args(args);
        command.output().expect("spawn pnpm")
    }

    /// [pnpm/pnpm#14976](https://github.com/pnpm/pnpm/issues/14976)
    #[test]
    fn deploy_runs_the_deploy_script_when_present() {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        workspace_with_member_scripts(
            &workspace,
            &serde_json::json!({ "deploy": "echo deploy-script-executed" }),
        );

        let output = run(&workspace, &["--dir", "packages/a", "deploy"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "pnpm deploy should succeed:\n{stdout}\n{stderr}");
        assert!(
            stdout.contains("deploy-script-executed"),
            "the deploy script should replace the built-in: {stdout}",
        );

        drop(root);
    }

    #[test]
    fn pm_deploy_runs_the_builtin_when_a_deploy_script_exists() {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        workspace_with_member_scripts(
            &workspace,
            &serde_json::json!({ "deploy": "echo deploy-script-executed" }),
        );

        let output = run(&workspace, &["pm", "deploy", "--filter", "a", "--dir", "packages/a"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "the built-in needs a target: {stdout}");
        assert!(!stdout.contains("deploy-script-executed"), "the script must not run: {stdout}");
        assert!(
            stderr.contains("ERR_PNPM_INVALID_DEPLOY_TARGET"),
            "the built-in must run and demand its target: {stderr}",
        );

        drop(root);
    }

    #[test]
    fn deploy_from_a_workspace_subdirectory_refuses_when_the_root_declares_the_script() {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        write_manifest(
            &workspace,
            "root-pkg",
            &serde_json::json!({ "deploy": "echo deploy-script-executed" }),
        );
        fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
            .expect("write pnpm-workspace.yaml");
        let member = workspace.join("packages").join("a");
        fs::create_dir_all(&member).expect("create the workspace member");
        write_manifest(&member, "a", &serde_json::json!({}));

        let output = run(&workspace, &["--dir", "packages/a", "deploy"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "pnpm deploy should fail: {stderr}");
        assert!(stderr.contains("ERR_PNPM_SCRIPT_OVERRIDE_IN_WORKSPACE_ROOT"), "stderr={stderr}");

        drop(root);
    }

    /// Each spelling looks up the script of its own name, like pnpm 11's
    /// redirect of the typed command.
    #[test]
    fn rebuild_and_rb_each_run_their_own_script() {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        workspace_with_member_scripts(
            &workspace,
            &serde_json::json!({
                "rebuild": "echo rebuild-script-ran",
                "rb": "echo rb-script-ran",
            }),
        );

        let output = run(&workspace, &["--dir", "packages/a", "rebuild"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "pnpm rebuild should succeed:\n{stdout}\n{stderr}");
        assert!(
            stdout.contains("rebuild-script-ran") && !stdout.contains("rb-script-ran"),
            "pnpm rebuild must run the rebuild script only: {stdout}",
        );

        let output = run(&workspace, &["--dir", "packages/a", "rb"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "pnpm rb should succeed:\n{stdout}\n{stderr}");
        assert!(
            stdout.contains("rb-script-ran") && !stdout.contains("rebuild-script-ran"),
            "pnpm rb must run the rb script only: {stdout}",
        );

        drop(root);
    }

    /// `HOME` and `PNPM_HOME` point into the temp dir, so a regression that
    /// reaches the built-in cannot self-install pnpm or edit the developer's
    /// shell rc files.
    #[test]
    fn setup_runs_the_setup_script_when_present() {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        workspace_with_member_scripts(
            &workspace,
            &serde_json::json!({ "setup": "echo setup-script-executed" }),
        );
        let mut command =
            Command::cargo_bin("pnpm").expect("find the pnpm binary").without_ambient_pnpm_config();
        command
            .current_dir(&workspace)
            .env("HOME", root.path())
            .env("PNPM_HOME", root.path().join("pnpm-home"))
            .args(["--dir", "packages/a", "setup"]);

        let output = command.output().expect("spawn pnpm");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "pnpm setup should succeed:\n{stdout}\n{stderr}");
        assert!(
            stdout.contains("setup-script-executed"),
            "the setup script should replace the built-in: {stdout}",
        );

        drop(root);
    }
}
