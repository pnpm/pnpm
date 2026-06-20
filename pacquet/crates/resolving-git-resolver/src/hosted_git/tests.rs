use super::{HostedGit, HostedGitType, HostedOpts};

#[test]
fn github_shortcut_user_repo() {
    let hosted = HostedGit::from_url("zkochan/is-negative").expect("recognised");
    assert_eq!(hosted.host_type, HostedGitType::Github);
    assert_eq!(hosted.user, "zkochan");
    assert_eq!(hosted.project, "is-negative");
    assert_eq!(hosted.committish, None);
}

#[test]
fn github_shortcut_with_commit() {
    let hosted =
        HostedGit::from_url("zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99")
            .expect("recognised");
    assert_eq!(hosted.committish.as_deref(), Some("163360a8d3ae6bee9524541043197ff356f8ed99"));
}

#[test]
fn github_colon_shortcut() {
    let hosted = HostedGit::from_url("github:zkochan/is-negative#canary").expect("recognised");
    assert_eq!(hosted.host_type, HostedGitType::Github);
    assert_eq!(hosted.user, "zkochan");
    assert_eq!(hosted.project, "is-negative");
    assert_eq!(hosted.committish.as_deref(), Some("canary"));
}

#[test]
fn https_full_url() {
    let hosted =
        HostedGit::from_url("https://github.com/zkochan/is-negative.git#2.0.1").expect("ok");
    assert_eq!(hosted.host_type, HostedGitType::Github);
    assert_eq!(hosted.user, "zkochan");
    assert_eq!(hosted.project, "is-negative");
    assert_eq!(hosted.committish.as_deref(), Some("2.0.1"));
}

#[test]
fn git_plus_ssh_url() {
    let hosted =
        HostedGit::from_url("git+ssh://git@github.com/zkochan/is-negative.git#2.0.1").expect("ok");
    assert_eq!(hosted.user, "zkochan");
    assert_eq!(hosted.project, "is-negative");
}

#[test]
fn bitbucket_shortcut() {
    let hosted = HostedGit::from_url("bitbucket:pnpmjs/git-resolver#0.3.4").expect("ok");
    assert_eq!(hosted.host_type, HostedGitType::Bitbucket);
    assert_eq!(hosted.user, "pnpmjs");
    assert_eq!(hosted.project, "git-resolver");
    assert_eq!(hosted.committish.as_deref(), Some("0.3.4"));
}

#[test]
fn gitlab_shortcut() {
    let hosted = HostedGit::from_url("gitlab:pnpm/git-resolver").expect("ok");
    assert_eq!(hosted.host_type, HostedGitType::Gitlab);
    assert_eq!(hosted.user, "pnpm");
    assert_eq!(hosted.project, "git-resolver");
}

#[test]
fn https_gitlab_url() {
    let hosted = HostedGit::from_url("https://gitlab.com/pnpmjs/git-resolver").expect("ok");
    assert_eq!(hosted.host_type, HostedGitType::Gitlab);
    assert_eq!(hosted.user, "pnpmjs");
    assert_eq!(hosted.project, "git-resolver");
}

#[test]
fn rejects_non_hosted() {
    assert!(HostedGit::from_url("https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git").is_none());
}

#[test]
fn rejects_random_string() {
    assert!(HostedGit::from_url("not-a-url").is_none());
    assert!(HostedGit::from_url("").is_none());
}

#[test]
fn rejects_relative_path() {
    // Starts with `.`, fails isGitHubShorthand.
    assert!(HostedGit::from_url("./local-dep").is_none());
}

#[test]
fn shortcut_render() {
    let hosted =
        HostedGit::from_url("zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99")
            .expect("ok");
    assert_eq!(
        hosted.shortcut(HostedOpts::default()),
        "github:zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99",
    );
    assert_eq!(hosted.shortcut(HostedGit::no_committish()), "github:zkochan/is-negative");
}

#[test]
fn https_render_with_commit() {
    let hosted = HostedGit::from_url("zkochan/is-negative").expect("ok");
    assert_eq!(
        hosted.https(HostedOpts::default()).unwrap(),
        "git+https://github.com/zkochan/is-negative.git",
    );
    assert_eq!(
        hosted.https(HostedGit::no_committish_no_git_plus()).unwrap(),
        "https://github.com/zkochan/is-negative.git",
    );
}

#[test]
fn ssh_render() {
    let hosted = HostedGit::from_url("foo/bar").expect("ok");
    assert_eq!(hosted.ssh(HostedOpts::default()).unwrap(), "git@github.com:foo/bar.git");
    assert_eq!(
        hosted.sshurl(HostedOpts::default()).unwrap(),
        "git+ssh://git@github.com/foo/bar.git",
    );
    assert_eq!(
        hosted.sshurl(HostedGit::no_committish()).unwrap(),
        "git+ssh://git@github.com/foo/bar.git",
    );
}

#[test]
fn tarball_github() {
    let mut hosted = HostedGit::from_url("zkochan/is-negative").expect("ok");
    hosted.committish = Some("163360a8d3ae6bee9524541043197ff356f8ed99".to_string());
    assert_eq!(
        hosted.tarball(HostedOpts::default()).unwrap(),
        "https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99",
    );
}

#[test]
fn tarball_bitbucket() {
    let mut hosted = HostedGit::from_url("bitbucket:foo/bar").expect("ok");
    hosted.committish = Some("abc123".to_string());
    assert_eq!(
        hosted.tarball(HostedOpts::default()).unwrap(),
        "https://bitbucket.org/foo/bar/get/abc123.tar.gz",
    );
}

#[test]
fn tarball_gitlab_uses_archive_path() {
    // Regression for pnpm <https://github.com/pnpm/pnpm/issues/11533>: the tarball must not embed
    // `%2F`. The `/-/archive/<ref>/<project>-<ref>.tar.gz` form
    // doesn't.
    let mut hosted = HostedGit::from_url("gitlab:pnpmjs/git-resolver").expect("ok");
    hosted.committish = Some("988c61e11dc8d9ca0b5580cb15291951812549dc".to_string());
    let tarball = hosted.tarball(HostedOpts::default()).unwrap();
    assert!(!tarball.contains("%2F"), "tarball must not contain `%2F`: {tarball}");
    assert_eq!(
        tarball,
        "https://gitlab.com/pnpmjs/git-resolver/-/archive/988c61e11dc8d9ca0b5580cb15291951812549dc/git-resolver-988c61e11dc8d9ca0b5580cb15291951812549dc.tar.gz",
    );
}

#[test]
fn tarball_returns_none_when_no_committish() {
    let hosted = HostedGit::from_url("zkochan/is-negative").expect("ok");
    assert!(hosted.tarball(HostedOpts::default()).is_none());
}

#[test]
fn https_with_auth() {
    let hosted = HostedGit::from_url(
        "git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git",
    )
    .expect("ok");
    assert_eq!(
        hosted.https(HostedGit::no_committish_no_git_plus()).unwrap(),
        "https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git",
    );
    assert!(hosted.auth.is_some());
}

#[test]
fn percent_decode_reassembles_utf8_sequences() {
    // `%E2%80%A6` is U+2026 (ellipsis) in UTF-8. A byte-wise
    // decoder would emit two Latin-1 chars; a UTF-8-aware decoder
    // restores the original ellipsis.
    assert_eq!(super::percent_decode("a%E2%80%A6b"), "a\u{2026}b");
    // Branch / tag with a percent-encoded scope-style slash
    // (`@foo/bar` → `%40foo%2Fbar`).
    assert_eq!(super::percent_decode("%40foo%2Fbar"), "@foo/bar");
}
