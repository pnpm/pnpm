use super::{
    has_configured_ssh_command,
    non_interactive_git_env,
};
use crate::{
    CommandOutput,
    RunCommand,
};
use std::{
    io,
    path::Path,
};

#[test]
fn disables_git_and_ssh_prompts() {
    assert_eq!(
        non_interactive_git_env(|_| false, || false),
        [("GIT_TERMINAL_PROMPT", "0"), ("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")],
    );
}

#[test]
fn keeps_an_ssh_command_selected_through_the_environment() {
    let unreachable = || unreachable!("the environment already selects the ssh command");
    assert_eq!(
        non_interactive_git_env(|name| name == "GIT_SSH_COMMAND", unreachable),
        [("GIT_TERMINAL_PROMPT", "0")],
    );
    assert_eq!(
        non_interactive_git_env(|name| name == "GIT_SSH", unreachable),
        [("GIT_TERMINAL_PROMPT", "0")],
    );
}

#[test]
fn keeps_an_ssh_command_selected_through_git_configuration() {
    assert_eq!(non_interactive_git_env(|_| false, || true), [("GIT_TERMINAL_PROMPT", "0")]);
}

struct SshCommandConfigured;

impl RunCommand for SshCommandConfigured {
    fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> io::Result<CommandOutput> {
        assert_eq!(
            (program, args, cwd),
            ("git", &["config", "--get", "core.sshCommand"][..], Some(Path::new("/repo"))),
        );
        Ok(CommandOutput {
            success: true,
            stdout: "ssh -i deploy_key\n".to_owned(),
            stderr: String::new(),
        })
    }
}

/// A provider whose `git config --get` exits 1, which is how git reports an
/// unset key.
struct SshCommandUnset;

impl RunCommand for SshCommandUnset {
    fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
        Ok(CommandOutput { success: false, stdout: String::new(), stderr: String::new() })
    }
}

struct NoGit;

impl RunCommand for NoGit {
    fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

#[test]
fn reads_the_ssh_command_setting_from_git_configuration() {
    assert!(has_configured_ssh_command::<SshCommandConfigured>(Some(Path::new("/repo"))));
    assert!(!has_configured_ssh_command::<SshCommandUnset>(None));
    assert!(!has_configured_ssh_command::<NoGit>(None));
}
