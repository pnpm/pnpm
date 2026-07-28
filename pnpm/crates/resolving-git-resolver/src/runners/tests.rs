use super::{LsRemoteMode, ls_remote_command};

fn args(mode: LsRemoteMode<'_>) -> Vec<String> {
    ls_remote_command(None, "--upload-pack=malicious", mode)
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn probe_separates_options_from_the_repository() {
    assert_eq!(
        args(LsRemoteMode::Probe),
        ["ls-remote", "--exit-code", "--", "--upload-pack=malicious", "HEAD"],
    );
}

#[test]
fn resolve_separates_options_from_the_repository_and_ref() {
    assert_eq!(
        args(LsRemoteMode::Resolve(Some("--help"))),
        ["ls-remote", "--", "--upload-pack=malicious", "--help", "--help^{}",],
    );
}

#[test]
fn passes_git_terminal_prompt_zero() {
    let cmd = ls_remote_command(None, "some-repo", LsRemoteMode::Probe);
    let mut has_env = false;
    for (k, v) in cmd.get_envs() {
        if k == "GIT_TERMINAL_PROMPT" {
            assert_eq!(v, Some(std::ffi::OsStr::new("0")));
            has_env = true;
        }
    }
    assert!(has_env);
}
