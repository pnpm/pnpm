use super::execute_shell;

/// `execute_shell` returns `Ok(())` when `sh -c <command>` runs
/// to completion. The whole `tests` module is `cfg(unix)`-gated
/// at the declaration site because `execute_shell` spawns `sh`
/// unconditionally and has no Windows equivalent worth covering
/// here.
#[test]
fn execute_shell_runs_successful_command() {
    execute_shell("true").expect("`true` runs cleanly");
}
