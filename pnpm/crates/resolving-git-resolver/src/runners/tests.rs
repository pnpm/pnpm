use std::sync::Arc;

use pacquet_network::ThrottledClient;

use super::{RealGitProbe, ls_remote_command};
use crate::git_resolver::GitProbe;

fn args(ref_: Option<&str>) -> Vec<String> {
    ls_remote_command(None, "--upload-pack=malicious", ref_)
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn resolve_separates_options_from_the_repository_and_ref() {
    assert_eq!(
        args(Some("--help")),
        ["ls-remote", "--", "--upload-pack=malicious", "--help", "--help^{}",],
    );
}

#[test]
fn passes_git_terminal_prompt_zero() {
    let cmd = ls_remote_command(None, "some-repo", None);
    let mut has_env = false;
    for (k, v) in cmd.get_envs() {
        if k == "GIT_TERMINAL_PROMPT" {
            assert_eq!(v, Some(std::ffi::OsStr::new("0")));
            has_env = true;
        }
    }
    assert!(has_env);
}

fn real_probe() -> RealGitProbe {
    RealGitProbe::new(Arc::new(ThrottledClient::default()))
}

#[tokio::test]
async fn head_probe_accepts_success_without_retrying() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("HEAD", "/foo/bar/tar.gz/abc").with_status(200).expect(1).create_async().await;
    assert!(real_probe().anonymous_head_ok(&format!("{}/foo/bar/tar.gz/abc", server.url())).await);
    mock.assert_async().await;
}

#[tokio::test]
async fn head_probe_does_not_retry_definitive_statuses() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("HEAD", "/foo/bar/tar.gz/abc").with_status(404).expect(1).create_async().await;
    assert!(!real_probe().anonymous_head_ok(&format!("{}/foo/bar/tar.gz/abc", server.url())).await);
    mock.assert_async().await;
}

#[tokio::test]
async fn head_probe_retries_transient_statuses_to_exhaustion() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("HEAD", "/foo/bar/tar.gz/abc").with_status(429).expect(3).create_async().await;
    assert!(!real_probe().anonymous_head_ok(&format!("{}/foo/bar/tar.gz/abc", server.url())).await);
    mock.assert_async().await;
}
