//! The tests that pin the contract in
//! `pnpm/crates/executor/PROCESS_LIFETIME.md`, which lists the tests behind
//! each of its rows. A change that makes one of them fail changes the
//! contract, and the contract has to change first.
//!
//! Most cases are differential: the fixture runs once without pnpm and once
//! under it, and the same leftover processes have to survive both runs.

use command_extra::CommandExtra;
use std::{
    fs,
    path::Path,
    process::Command,
    thread::sleep,
    time::{Duration, Instant},
};

mod breakaway;
mod interrupt;
mod killed;
mod multi_task;
mod single_command;

/// `node waiter.js <marker>` writes `<marker>.up` as soon as it runs, then
/// writes `<marker>` once `release.txt` exists in its working directory, so
/// a marker written after the release proves the waiter outlived whatever
/// happened before it. It ignores `Ctrl+C`, as a program that has nothing
/// to do with the terminal's job does, and dies of a hangup like any
/// program. It gives up after 30 seconds, so a failed test leaves nothing
/// running for long.
const WAITER: &str = r"const fs = require('fs')
const marker = process.argv[2]
process.on('SIGINT', () => {})
fs.writeFileSync(marker + '.up', '')
const deadline = Date.now() + 30000
const wait = () => fs.existsSync('release.txt') ? fs.writeFileSync(marker, '') : Date.now() < deadline && setTimeout(wait, 50)
wait()
";

/// `node launcher.js <then> <leftover>:<marker>...` starts a [`WAITER`] for
/// each leftover and waits until each is up. Then it does what `then` says:
/// exit with that code, `wait` to be ended (writing `started.txt`), or
/// `hold`, which waits too but ignores `Ctrl+C`.
const LAUNCHER: &str = r#"#!/usr/bin/env node
const { spawn, spawnSync } = require('child_process')
const fs = require('fs')
const waiter = require('path').join(__dirname, 'waiter.js')
const [then, ...leftovers] = process.argv.slice(2)
const markers = []
for (const leftover of leftovers) {
  const [kind, marker] = leftover.split(':')
  markers.push(marker)
  if (kind === 'detached') {
    spawn(process.execPath, [waiter, marker], { detached: true, stdio: 'ignore' }).unref()
  } else if (process.platform === 'win32') {
    // A child of Node's own would sit in libuv's job and die with Node, so
    // the program is started the way a script starts one, with `start /b`.
    const command = `/d /s /c "start "" /b "${process.execPath}" "${waiter}" "${marker}""`
    spawnSync('cmd.exe', [command], { windowsVerbatimArguments: true, stdio: 'ignore' })
  } else {
    spawn(process.execPath, [waiter, marker], { stdio: 'ignore' }).unref()
  }
}
const up = () => markers.every(marker => fs.existsSync(marker + '.up'))
if (then === 'wait' || then === 'hold') {
  if (then === 'hold') process.on('SIGINT', () => {})
  const ready = () => up() ? fs.writeFileSync('started.txt', '') : setTimeout(ready, 20)
  ready()
  setTimeout(() => process.exit(3), 60000)
} else {
  const deadline = Date.now() + 10000
  while (!up() && Date.now() < deadline) {
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 20)
  }
  process.exit(Number(then))
}
"#;

/// What a fixture leaves running.
#[derive(Clone, Copy, Debug)]
enum Leftover {
    /// A detached child: in a session of its own on Unix, without a console
    /// on Windows. Neither `Ctrl+C` nor a closing terminal reaches it.
    Detached,
    /// A program still attached to the terminal: in the launcher's process
    /// group on Unix, and on its console on Windows, where `start /b`
    /// started it. A closing terminal reaches it.
    Attached,
}

impl Leftover {
    fn kind(self) -> &'static str {
        match self {
            Self::Detached => "detached",
            Self::Attached => "attached",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Detached => "detached-marker.txt",
            Self::Attached => "attached-marker.txt",
        }
    }
}

/// Write [`LAUNCHER`] and [`WAITER`] into `dir`.
fn write_fixture(dir: &Path) {
    fs::write(dir.join("launcher.js"), LAUNCHER).expect("write launcher.js");
    fs::write(dir.join("waiter.js"), WAITER).expect("write waiter.js");
}

/// The arguments that make [`LAUNCHER`] start `leftovers` and then do
/// `then`.
fn launcher_args(then: &str, leftovers: &[Leftover]) -> Vec<String> {
    std::iter::once(then.to_string())
        .chain(
            leftovers
                .iter()
                .map(|leftover| format!("{}:{}", leftover.kind(), leftover.marker())),
        )
        .collect()
}

/// `node` running [`LAUNCHER`] with `args` in `dir`, without pnpm.
fn launcher_directly(dir: &Path, args: &[String]) -> Command {
    Command::new(node_executable())
        .with_current_dir(dir)
        .with_arg("launcher.js")
        .with_args(args)
}

/// Release the waiters in `dir` and report, for each of `leftovers`, whether
/// it is still running.
fn surviving(dir: &Path, leftovers: &[Leftover]) -> Vec<bool> {
    fs::write(dir.join("release.txt"), "").expect("release the waiters");
    leftovers
        .iter()
        .map(|leftover| wait_for_file(&dir.join(leftover.marker())))
        .collect()
}

/// Wait up to ten seconds for `path` to appear.
fn wait_for_file(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(50));
    }
    true
}

/// The Node executable itself. `node` on `PATH` may be a shim that runs the
/// real one as its child, and the shim reacts to `Ctrl+C` and to being
/// killed on its own terms, so every fixture runs the real one.
fn node_executable() -> &'static std::path::Path {
    static NODE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    NODE.get_or_init(|| {
        let output = Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .expect("ask node for its executable");
        let path = String::from_utf8(output.stdout).expect("node prints its path as UTF-8");
        std::path::PathBuf::from(path.trim())
    })
}

/// Launch `pacquet` the way `nr` from `@antfu/ni` does: as a direct child of
/// Node without a shell, which places it in libuv's Job Object. `on_close` is
/// the JavaScript run with pacquet's exit `code` once it exits.
#[cfg(windows)]
fn node_launching_pacquet(pacquet: &Command, args: &[&str], on_close: &str) -> Command {
    let pacquet_path_json = serde_json::to_string(&pacquet.get_program().to_string_lossy())
        .expect("quote pacquet path");
    let args_json = serde_json::to_string(args).expect("quote pacquet args");
    let mut node = Command::new(node_executable());
    node.arg("-e").arg(format!(
        "const child = require('child_process').spawn({pacquet_path_json}, {args_json}, {{ stdio: 'inherit' }}); child.on('close', code => {{ {on_close} }})",
    ));
    if let Some(dir) = pacquet.get_current_dir() {
        node.current_dir(dir);
    }
    for (name, value) in pacquet.get_envs() {
        match value {
            Some(value) => node.env(name, value),
            None => node.env_remove(name),
        };
    }
    node
}
