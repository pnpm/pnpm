//! The single-command half of the contract in
//! `pnpm/crates/executor/PROCESS_LIFETIME.md`: what a command leaves running
//! outlives `pnpm exec` exactly when it outlives the command run without
//! pnpm. Every case runs its fixture both ways and compares.
//!
//! The cases a bail cancels, and pnpm killed while its command runs, are
//! where pnpm differs on purpose; those tests sit with the commands they
//! cover.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::Path,
    process::Command,
    thread::sleep,
    time::{Duration, Instant},
};

/// `node launcher.js <leftover> <marker> <release> <code>` starts a leftover
/// process, then exits with `code`. The leftover is either a detached child
/// or, on Windows, a program `start` launched, which is how scripts open
/// editors and browsers.
const LAUNCHER: &str = r#"const { spawn, spawnSync } = require('child_process')
const waiter = require('path').join(__dirname, 'waiter.js')
const [leftover, marker, release, code] = process.argv.slice(2)
if (leftover === 'detached') {
  spawn(process.execPath, [waiter, marker, release], { detached: true, stdio: 'ignore' }).unref()
} else {
  const command = `/d /s /c "start "" /b "${process.execPath}" "${waiter}" "${marker}" "${release}""`
  spawnSync('cmd.exe', [command], { windowsVerbatimArguments: true, stdio: 'ignore' })
}
process.exit(Number(code))
"#;

/// `node waiter.js <marker> <release>` writes `marker` once `release` exists,
/// so a marker after the release proves the waiter outlived its launcher.
const WAITER: &str = r"const fs = require('fs')
const [marker, release] = process.argv.slice(2)
const deadline = Date.now() + 30000
const wait = () => fs.existsSync(release) ? fs.writeFileSync(marker, '') : Date.now() < deadline && setTimeout(wait, 50)
wait()
";

#[derive(Clone, Copy, Debug)]
enum Leftover {
    Detached,
    #[cfg(windows)]
    Started,
}

impl Leftover {
    fn arg(self) -> &'static str {
        match self {
            Self::Detached => "detached",
            #[cfg(windows)]
            Self::Started => "start",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Launch {
    Directly,
    UnderPnpm,
    /// pnpm launched by Node, which puts pnpm in libuv's job, as `nr` does.
    #[cfg(windows)]
    UnderPnpmFromNode,
}

/// Run the launcher the way `launch` says and report whether its leftover
/// outlived it.
fn leftover_survives(launch: Launch, leftover: Leftover, code: i32) -> bool {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("launcher.js"), LAUNCHER).expect("write launcher.js");
    fs::write(workspace.join("waiter.js"), WAITER).expect("write waiter.js");
    let marker = workspace.join("marker.txt");
    let release = workspace.join("release.txt");
    let launcher_args = [
        "launcher.js".to_string(),
        leftover.arg().to_string(),
        marker.to_string_lossy().into_owned(),
        release.to_string_lossy().into_owned(),
        code.to_string(),
    ];

    let mut command = match launch {
        Launch::Directly => {
            let mut node = Command::new("node");
            node.current_dir(&workspace).args(&launcher_args);
            node
        }
        Launch::UnderPnpm => pacquet
            .with_args(["exec", "node"])
            .with_args(&launcher_args),
        #[cfg(windows)]
        Launch::UnderPnpmFromNode => {
            let args: Vec<&str> = ["exec", "node"]
                .into_iter()
                .chain(launcher_args.iter().map(String::as_str))
                .collect();
            crate::exec::node_launching_pacquet(&pacquet, &args, "process.exit(code)")
        }
    };
    let status = command.status().expect("run the launcher");
    assert_eq!(status.code(), Some(code), "{launch:?} must end as the launcher did");

    fs::write(&release, "").expect("release the leftover");
    let survived = wait_for(&marker);
    drop(root);
    survived
}

fn wait_for(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(50));
    }
    true
}

/// The leftover survives the command run directly, which keeps a fixture
/// that never starts it from passing, and survives it under pnpm the same.
fn assert_survives_as_without_pnpm(launch: Launch, leftover: Leftover, code: i32) {
    assert!(
        leftover_survives(Launch::Directly, leftover, code),
        "the {leftover:?} leftover must outlive the command run directly",
    );
    assert!(
        leftover_survives(launch, leftover, code),
        "the {leftover:?} leftover must outlive a command ending with {code} under {launch:?}, as it does without pnpm",
    );
}

#[test]
fn a_detached_process_outlives_a_successful_command() {
    assert_survives_as_without_pnpm(Launch::UnderPnpm, Leftover::Detached, 0);
}

#[test]
fn a_detached_process_outlives_a_failed_command() {
    assert_survives_as_without_pnpm(Launch::UnderPnpm, Leftover::Detached, 1);
}

#[cfg(windows)]
#[test]
fn a_detached_process_outlives_a_successful_command_when_node_launches_pnpm() {
    assert_survives_as_without_pnpm(Launch::UnderPnpmFromNode, Leftover::Detached, 0);
}

#[cfg(windows)]
#[test]
fn a_detached_process_outlives_a_failed_command_when_node_launches_pnpm() {
    assert_survives_as_without_pnpm(Launch::UnderPnpmFromNode, Leftover::Detached, 1);
}

/// pnpm/pnpm#14356
#[cfg(windows)]
#[test]
fn a_started_program_outlives_a_successful_command() {
    assert_survives_as_without_pnpm(Launch::UnderPnpm, Leftover::Started, 0);
}

#[cfg(windows)]
#[test]
fn a_started_program_outlives_a_failed_command() {
    assert_survives_as_without_pnpm(Launch::UnderPnpm, Leftover::Started, 1);
}
