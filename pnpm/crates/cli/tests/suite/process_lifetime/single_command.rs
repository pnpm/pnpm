//! A single command that ends on its own, successfully or not: what it left
//! running outlives pnpm exactly as it outlives the command run without
//! pnpm, whichever way pnpm runs the command.

use super::{Leftover, launcher_args, launcher_directly, surviving, write_fixture};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, path::Path, process::Command};

/// How pnpm runs the launcher.
#[derive(Clone, Copy, Debug)]
enum Launch {
    Exec,
    /// `pnpm run`, through the script shell.
    Run,
    /// A `postinstall` script of the project, under `pnpm install`.
    Install,
    /// The bin of a package `pnpm dlx` installs.
    Dlx,
    /// `pnpm exec`, with pnpm launched by Node.
    #[cfg(windows)]
    ExecFromNode,
}

const LEFTOVERS: [Leftover; 2] = [Leftover::Detached, Leftover::Attached];

impl Launch {
    /// The command that has pnpm run the launcher with `args` in `dir`,
    /// which `pacquet` already runs in.
    fn command(self, pacquet: Command, dir: &Path, args: &[String]) -> Command {
        let script = format!("node launcher.js {}", args.join(" "));
        match self {
            Self::Exec => pacquet
                .with_arg("exec")
                .with_arg(super::node_executable())
                .with_arg("launcher.js")
                .with_args(args),
            Self::Run => {
                write_manifest(dir, &json!({ "launch": script }));
                pacquet.with_args(["--config.verify-deps-before-run=false", "run", "launch"])
            }
            Self::Install => {
                write_manifest(dir, &json!({ "postinstall": script }));
                pacquet.with_arg("install")
            }
            Self::Dlx => {
                let package = dir.join("launcher-package");
                fs::create_dir(&package).expect("create the dlx package");
                write_fixture(&package);
                let manifest = json!({
                    "name": "launcher-package",
                    "version": "1.0.0",
                    "bin": { "launch": "launcher.js" },
                });
                fs::write(package.join("package.json"), manifest.to_string())
                    .expect("write the dlx package's manifest");
                pacquet
                    .with_arg("dlx")
                    .with_arg(format!("--package=file:{}", package.display()))
                    .with_arg("launch")
                    .with_args(args)
            }
            #[cfg(windows)]
            Self::ExecFromNode => {
                let node = super::node_executable().to_string_lossy();
                let args: Vec<&str> = ["exec", &node, "launcher.js"]
                    .into_iter()
                    .chain(args.iter().map(String::as_str))
                    .collect();
                super::node_launching_pacquet(&pacquet, &args, "process.exit(code)")
            }
        }
    }
}

fn write_manifest(dir: &Path, scripts: &serde_json::Value) {
    let manifest = json!({ "name": "fixture", "version": "1.0.0", "scripts": scripts });
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// Which of [`LEFTOVERS`] outlive the launcher ending with `code`, run
/// directly when `launch` is `None` and the way `launch` says otherwise.
fn survivors(launch: Option<Launch>, code: i32) -> Vec<bool> {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_fixture(&workspace);
    let args = launcher_args(&code.to_string(), &LEFTOVERS);
    let mut command = match launch {
        None => launcher_directly(&workspace, &args),
        Some(launch) => launch.command(pacquet, &workspace, &args),
    };
    let output = command.output().expect("run the launcher");
    assert_eq!(
        output.status.success(),
        code == 0,
        "{launch:?} must succeed exactly when the launcher does: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let survivors = surviving(&workspace, &LEFTOVERS);
    drop(root);
    survivors
}

/// Every leftover survives the launcher run directly, which keeps a fixture
/// that never starts one from passing, and they survive it under pnpm the
/// same.
fn assert_survive_as_without_pnpm(launch: Launch, code: i32) {
    let without_pnpm = survivors(None, code);
    assert_eq!(without_pnpm, [true, true], "{LEFTOVERS:?} outlive the launcher run directly");
    assert_eq!(
        survivors(Some(launch), code),
        without_pnpm,
        "{LEFTOVERS:?} must outlive a command ending with {code} under {launch:?} as they do without pnpm",
    );
}

#[test]
fn leftovers_outlive_a_successful_exec() {
    assert_survive_as_without_pnpm(Launch::Exec, 0);
}

#[test]
fn leftovers_outlive_a_failed_exec() {
    assert_survive_as_without_pnpm(Launch::Exec, 1);
}

#[test]
fn leftovers_outlive_a_successful_run() {
    assert_survive_as_without_pnpm(Launch::Run, 0);
}

#[test]
fn leftovers_outlive_a_failed_run() {
    assert_survive_as_without_pnpm(Launch::Run, 1);
}

#[test]
fn leftovers_outlive_a_successful_lifecycle_script() {
    assert_survive_as_without_pnpm(Launch::Install, 0);
}

#[test]
fn leftovers_outlive_a_failed_lifecycle_script() {
    assert_survive_as_without_pnpm(Launch::Install, 1);
}

#[test]
fn leftovers_outlive_a_successful_dlx() {
    assert_survive_as_without_pnpm(Launch::Dlx, 0);
}

#[test]
fn leftovers_outlive_a_failed_dlx() {
    assert_survive_as_without_pnpm(Launch::Dlx, 1);
}

#[cfg(windows)]
#[test]
fn leftovers_outlive_a_successful_exec_when_node_launches_pnpm() {
    assert_survive_as_without_pnpm(Launch::ExecFromNode, 0);
}

#[cfg(windows)]
#[test]
fn leftovers_outlive_a_failed_exec_when_node_launches_pnpm() {
    assert_survive_as_without_pnpm(Launch::ExecFromNode, 1);
}
