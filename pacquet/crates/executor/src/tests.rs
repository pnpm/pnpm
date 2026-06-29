// `execute_shell` spawns `sh` and has no Windows equivalent worth covering,
// so the whole test module is gated to Unix.
#![cfg(unix)]

use super::execute_shell;

#[test]
fn execute_shell_runs_successful_command() {
    execute_shell("true").expect("`true` runs cleanly");
}
