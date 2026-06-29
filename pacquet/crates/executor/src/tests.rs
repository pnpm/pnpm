use super::execute_shell;

#[test]
fn execute_shell_runs_successful_command() {
    execute_shell("true").expect("`true` runs cleanly");
}
