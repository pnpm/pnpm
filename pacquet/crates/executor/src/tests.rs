// `execute_shell` spawns `sh` unconditionally and has no Windows
// equivalent worth covering, so the whole module is Unix-only. The gate
// lives here (rather than `#[cfg(all(test, unix))]` on the `mod tests;`
// declaration) so `unit_test_file_layout` still sees a plain
// `#[cfg(test)] mod tests;` and recognises this file as the external
// test module.
#![cfg(unix)]

use super::execute_shell;

#[test]
fn execute_shell_runs_successful_command() {
    execute_shell("true").expect("`true` runs cleanly");
}
