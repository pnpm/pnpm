//! A process created with `CREATE_BREAKAWAY_FROM_JOB` leaves pnpm's Job
//! Object, the one exemption from it (pnpm/pnpm#16628). Windows only by
//! subject.
#![cfg(windows)]

use super::{node_launching_pacquet, wait_for_file};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path};

/// PowerShell run with a mode argument:
///
/// - `fail`: start a breakaway waiter, then exit 1.
/// - `detach`: start this script in `late` mode without breakaway, so it stays
///   in pacquet's job, then exit 0. It gets `CREATE_NO_WINDOW` because
///   PowerShell exits at once under `DETACHED_PROCESS`.
/// - `late`: wait for `$env:BREAKAWAY_RELEASE`, then start a breakaway waiter.
///
/// The breakaway waiter is a Node process started with
/// `CREATE_BREAKAWAY_FROM_JOB` that writes `$env:BREAKAWAY_MARKER` once
/// `$env:BREAKAWAY_RELEASE` exists. A refused spawn prints the Win32 error
/// and exits 2.
const BREAKAWAY_SPAWN_SCRIPT: &str = r#"
param([string]$Mode)
Add-Type -Name P -Namespace W -MemberDefinition @"
[StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct SI { public int cb; public string r, d, t; public int x,y,xs,ys,xc,yc,fa,fl; public short sw, r2; public IntPtr r3, i, o, e; }
[StructLayout(LayoutKind.Sequential)] public struct PI { public IntPtr hp, ht; public int pid, tid; }
[DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] public static extern bool CreateProcessW(string app, System.Text.StringBuilder cmd, IntPtr pa, IntPtr ta, bool inheritHandles, uint flags, IntPtr env, string cwd, ref SI si, out PI pi);
"@
function New-ChildProcess([string]$cmd, [uint32]$flags) {
  $si = New-Object W.P+SI; $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si); $pi = New-Object W.P+PI
  $ok = [W.P]::CreateProcessW([NullString]::Value, [Text.StringBuilder]$cmd, [IntPtr]::Zero, [IntPtr]::Zero, $false, $flags, [IntPtr]::Zero, [NullString]::Value, [ref]$si, [ref]$pi)
  if (-not $ok) { "CreateProcessW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"; exit 2 }
}
$DETACHED_PROCESS = 0x8
$CREATE_NEW_PROCESS_GROUP = 0x200
$CREATE_BREAKAWAY_FROM_JOB = 0x01000000
$CREATE_NO_WINDOW = 0x08000000
$waiter = '"' + (Get-Command node).Source + '" -e "const fs = require(`fs`); const deadline = Date.now() + 30000; const wait = () => fs.existsSync(process.env.BREAKAWAY_RELEASE) ? fs.writeFileSync(process.env.BREAKAWAY_MARKER, ``) : Date.now() < deadline && setTimeout(wait, 50); wait()"'
$breakawayFlags = $DETACHED_PROCESS -bor $CREATE_NEW_PROCESS_GROUP -bor $CREATE_BREAKAWAY_FROM_JOB
switch ($Mode) {
  'fail' { New-ChildProcess $waiter $breakawayFlags; exit 1 }
  'detach' {
    $late = '"' + (Get-Process -Id $PID).Path + '" -NoProfile -ExecutionPolicy Bypass -File "' + $PSCommandPath + '" late'
    New-ChildProcess $late ($CREATE_NO_WINDOW -bor $CREATE_NEW_PROCESS_GROUP)
    exit 0
  }
  'late' {
    $deadline = (Get-Date).AddSeconds(30)
    while (-not (Test-Path $env:BREAKAWAY_RELEASE) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50 }
    New-ChildProcess $waiter $breakawayFlags
  }
}
"#;

/// [`BREAKAWAY_SPAWN_SCRIPT`] written to a workspace, with the files it
/// coordinates through.
struct BreakawayFixture {
    script: std::path::PathBuf,
    release: std::path::PathBuf,
    marker: std::path::PathBuf,
}

impl BreakawayFixture {
    fn write(workspace: &Path) -> Self {
        let fixture = BreakawayFixture {
            script: workspace.join("breakaway.ps1"),
            release: workspace.join("breakaway-release.txt"),
            marker: workspace.join("breakaway-marker.txt"),
        };
        fs::write(&fixture.script, BREAKAWAY_SPAWN_SCRIPT).expect("write breakaway script");
        fixture
    }

    fn configure(&self, pacquet: std::process::Command) -> std::process::Command {
        pacquet
            .with_env("BREAKAWAY_RELEASE", &self.release)
            .with_env("BREAKAWAY_MARKER", &self.marker)
    }

    fn exec_args(&self, mode: &str) -> Vec<String> {
        ["exec", "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
            .into_iter()
            .map(String::from)
            .chain([self.script.to_string_lossy().into_owned(), mode.to_string()])
            .collect()
    }

    /// Release the waiter after pacquet has exited, so a marker proves the
    /// waiter outlived pacquet's job.
    fn release_and_wait_for_marker(&self) -> bool {
        assert!(!self.marker.exists(), "the waiter must not write the marker before its release");
        fs::write(&self.release, "").expect("release breakaway waiter");
        wait_for_file(&self.marker)
    }
}

#[test]
fn exec_lets_a_breakaway_process_outlive_a_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let fixture = BreakawayFixture::write(&workspace);

    let output = fixture
        .configure(pacquet)
        .with_args(fixture.exec_args("fail"))
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "the breakaway spawn should succeed: {stdout}");

    assert!(
        fixture.release_and_wait_for_marker(),
        "the breakaway process should survive a failed pnpm exec",
    );

    drop(root);
}

#[test]
fn exec_lets_a_breakaway_process_outlive_a_failure_when_node_launches_pnpm() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let fixture = BreakawayFixture::write(&workspace);
    let pacquet = fixture.configure(pacquet);
    let args = fixture.exec_args("fail");
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .collect();

    let output = node_launching_pacquet(&pacquet, &args, "process.exit(code)")
        .output()
        .expect("spawn node launching pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "the breakaway spawn should succeed: {stdout}");

    assert!(
        fixture.release_and_wait_for_marker(),
        "the breakaway process should survive a failed pnpm exec launched from Node",
    );

    drop(root);
}

/// A detached process that survives a successful command stays in the
/// released job, which must still let its children break away.
#[test]
fn exec_lets_a_detached_process_break_away_after_success() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let fixture = BreakawayFixture::write(&workspace);

    let output = fixture
        .configure(pacquet)
        .with_args(fixture.exec_args("detach"))
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "the detached spawn should succeed: {stdout}");

    assert!(
        fixture.release_and_wait_for_marker(),
        "a detached process should be able to break away after a successful pnpm exec",
    );

    drop(root);
}
