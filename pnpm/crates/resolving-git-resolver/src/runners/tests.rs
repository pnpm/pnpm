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
