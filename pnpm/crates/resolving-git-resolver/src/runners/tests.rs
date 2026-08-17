use std::sync::Arc;

use pnpm_network::ThrottledClient;

use super::{RealGitProbe, RealGitRunner, ls_remote_command};
use crate::{git_resolver::GitProbe, resolve_ref::GitCommandRunner};

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
async fn head_probe_bounds_attempts_on_an_unresponsive_endpoint() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hold_connections_open = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let probe = RealGitProbe {
        http_client: Arc::new(ThrottledClient::default()),
        head_timeout: std::time::Duration::from_millis(100),
    };
    let started = std::time::Instant::now();
    assert!(!probe.anonymous_head_ok(&format!("http://{addr}/foo/tar.gz/abc")).await);
    // 3 timed-out attempts plus 1.5s of backoff; generous slack for CI
    // load. Without the per-attempt deadline this hangs for the
    // client-wide timeout per attempt instead.
    assert!(started.elapsed() < std::time::Duration::from_secs(15), "{:?}", started.elapsed());
    hold_connections_open.abort();
}

#[tokio::test]
async fn head_probe_retries_transient_statuses_to_exhaustion() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("HEAD", "/foo/bar/tar.gz/abc").with_status(429).expect(3).create_async().await;
    assert!(!real_probe().anonymous_head_ok(&format!("{}/foo/bar/tar.gz/abc", server.url())).await);
    mock.assert_async().await;
}

// Every production `RealGitRunner` leaves `git_bin` unset and spawns `git`
// from `PATH`, so a configured missing path is the portable way to make the
// spawn fail; the message asserted here is the one a user without git gets.
#[tokio::test]
async fn a_missing_git_binary_is_reported_as_one() {
    let runner = RealGitRunner { git_bin: Some("/nonexistent/git".into()) };

    let err = runner.ls_remote("https://github.com/foo/bar.git", None).await.unwrap_err();

    assert_eq!(
        err.to_string(),
        "git ls-remote failed: `git` executable not found on PATH. Install git to resolve git-hosted packages.",
    );
}
