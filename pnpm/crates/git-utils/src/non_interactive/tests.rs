use super::non_interactive_git_env;

#[test]
fn disables_git_and_ssh_prompts() {
    assert_eq!(
        non_interactive_git_env(|_| false),
        [("GIT_TERMINAL_PROMPT", "0"), ("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")],
    );
}

#[test]
fn keeps_a_user_configured_ssh_command() {
    assert_eq!(
        non_interactive_git_env(|name| name == "GIT_SSH_COMMAND"),
        [("GIT_TERMINAL_PROMPT", "0")],
    );
    assert_eq!(non_interactive_git_env(|name| name == "GIT_SSH"), [("GIT_TERMINAL_PROMPT", "0")]);
}
