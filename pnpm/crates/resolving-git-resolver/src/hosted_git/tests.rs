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
        hosted
            .https(HostedOpts::default())
            .unwrap(),
        "git+https://github.com/zkochan/is-negative.git",
    );
    assert_eq!(
        hosted
            .https(HostedGit::no_committish_no_git_plus())
            .unwrap(),
        "https://github.com/zkochan/is-negative.git",
    );
}

#[test]
fn docs_render() {
    assert_eq!(
        HostedGit::package_docs_url("gitlab:pnpmjs/git-resolver"),
        Some("https://gitlab.com/pnpmjs/git-resolver#readme".to_string()),
    );
    assert_eq!(
        HostedGit::package_docs_url("https://github.com/pugjs/pug/tree/master/packages/pug-attrs"),
        Some("https://github.com/pugjs/pug/tree/master#readme".to_string()),
    );
    assert_eq!(
        HostedGit::package_docs_url("https://github.com/user/repo/tree/feature%2Ffoo"),
        Some("https://github.com/user/repo/tree/feature%2Ffoo#readme".to_string()),
    );
    assert_eq!(
        HostedGit::package_docs_url("github:user/repo#feature/foo"),
        Some("https://github.com/user/repo/tree/feature%2Ffoo#readme".to_string()),
    );
}

#[test]
fn ssh_render() {
    let hosted = HostedGit::from_url("foo/bar").expect("ok");
    assert_eq!(
        hosted
            .ssh(HostedOpts::default())
            .unwrap(),
        "git@github.com:foo/bar.git",
    );
    assert_eq!(
        hosted
            .sshurl(HostedOpts::default())
            .unwrap(),
        "git+ssh://git@github.com/foo/bar.git",
    );
    assert_eq!(
        hosted
            .sshurl(HostedGit::no_committish())
            .unwrap(),
        "git+ssh://git@github.com/foo/bar.git",
    );
}

#[test]
fn tarball_github() {
    let mut hosted = HostedGit::from_url("zkochan/is-negative").expect("ok");
    hosted.committish = Some("163360a8d3ae6bee9524541043197ff356f8ed99".to_string());
    assert_eq!(
        hosted
            .tarball(HostedOpts::default())
            .unwrap(),
        "https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99",
    );
}

#[test]
fn tarball_bitbucket() {
    let mut hosted = HostedGit::from_url("bitbucket:foo/bar").expect("ok");
    hosted.committish = Some("abc123".to_string());
    assert_eq!(
        hosted
            .tarball(HostedOpts::default())
            .unwrap(),
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
    let tarball = hosted
        .tarball(HostedOpts::default())
        .unwrap();
    assert!(!tarball.contains("%2F"), "tarball must not contain `%2F`: {tarball}");
    assert_eq!(
        tarball,
        "https://gitlab.com/pnpmjs/git-resolver/-/archive/988c61e11dc8d9ca0b5580cb15291951812549dc/git-resolver-988c61e11dc8d9ca0b5580cb15291951812549dc.tar.gz",
    );
}

#[test]
fn tarball_returns_none_when_no_committish() {
    let hosted = HostedGit::from_url("zkochan/is-negative").expect("ok");
    assert!(
        hosted
            .tarball(HostedOpts::default())
            .is_none(),
    );
}

#[test]
fn https_with_auth() {
    let hosted = HostedGit::from_url(
        "git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git",
    )
    .expect("ok");
    assert_eq!(
        hosted
            .https(HostedGit::no_committish_no_git_plus())
            .unwrap(),
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

#[test]
fn synthesized_name_scopes_a_repository_by_its_owner() {
    for (specifier, expected) in [
        ("github:anthropics/skills", "@anthropics/skills"),
        ("github:vercel-labs/skills", "@vercel-labs/skills"),
        ("zkochan/is-negative#canary", "@zkochan/is-negative"),
        ("https://github.com/zkochan/is-negative.git#2.0.1", "@zkochan/is-negative"),
        ("bitbucket:pnpmjs/git-resolver", "@pnpmjs/git-resolver"),
    ] {
        let hosted = HostedGit::from_url(specifier).expect("recognised");
        assert_eq!(hosted.synthesized_package_name(), expected, "specifier: {specifier}");
    }
}

#[test]
fn synthesized_name_is_lowercased() {
    let hosted = HostedGit::from_url("github:Anthropics/Skills").expect("recognised");
    assert_eq!(hosted.synthesized_package_name(), "@anthropics/skills");
}

/// A GitLab project in a subgroup arrives with the groups joined by `/`,
/// which a scope cannot hold. The separator they join with has to be one
/// no group path can spell, or a nested path would take the name of a
/// top-level group.
#[test]
fn synthesized_name_joins_gitlab_subgroups() {
    let nested = HostedGit::from_url("gitlab:group/subgroup/project").expect("recognised");
    assert_eq!(nested.user, "group/subgroup");
    assert_eq!(nested.synthesized_package_name(), "@group~subgroup/project");

    let flat = HostedGit::from_url("gitlab:group-subgroup/project").expect("recognised");
    assert_ne!(flat.synthesized_package_name(), nested.synthesized_package_name());
    assert_eq!(flat.synthesized_package_name(), "@group-subgroup/project");
}

#[test]
fn synthesized_name_of_an_ownerless_shortcut_is_the_project() {
    let hosted = HostedGit::from_url("github:skills").expect("recognised");
    assert_eq!(hosted.user, "");
    assert_eq!(hosted.synthesized_package_name(), "skills");
}

#[test]
fn normalized_repository_url_matches_npm_pkg_fix() {
    for (url, expected) in [
        ("github:pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("https://github.com/pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("https://github.com/pnpm/example.git", "git+https://github.com/pnpm/example.git"),
        ("https://www.github.com/pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("git+https://github.com/pnpm/example.git", "git+https://github.com/pnpm/example.git"),
        ("git@github.com:pnpm/example.git", "git+ssh://git@github.com/pnpm/example.git"),
        ("ssh://git@github.com/pnpm/example.git", "git+ssh://git@github.com/pnpm/example.git"),
        ("git+ssh://git@github.com/pnpm/example.git", "git+ssh://git@github.com/pnpm/example.git"),
        ("http://github.com/pnpm/example", "git+ssh://git@github.com/pnpm/example.git"),
        ("git://github.com/pnpm/example.git", "git://github.com/pnpm/example.git"),
        ("git://github.com/pnpm/example.git#v1", "git://github.com/pnpm/example.git#v1"),
        ("gitlab:foo/bar", "git+https://gitlab.com/foo/bar.git"),
        ("https://gitlab.com/group/sub/proj", "git+https://gitlab.com/group/sub/proj.git"),
        ("bitbucket:foo/bar", "git+https://bitbucket.org/foo/bar.git"),
        ("github:pnpm/example#v1", "git+https://github.com/pnpm/example.git#v1"),
        ("github:pnpm/example#semver:^1", "git+https://github.com/pnpm/example.git#semver:^1"),
        ("gist:abc123", "git+https://gist.github.com/abc123.git"),
        ("gist:abc123#deadbeef", "git+https://gist.github.com/abc123.git#deadbeef"),
        ("https://gist.github.com/foo/abc123", "git+https://gist.github.com/abc123.git"),
        ("git+ssh://git@gist.github.com/abc123.git", "git+ssh://git@gist.github.com/abc123.git"),
        ("git://gist.github.com/abc123.git", "git://gist.github.com/abc123.git"),
    ] {
        let hosted = HostedGit::from_url(url).expect("recognised");
        assert_eq!(hosted.normalized_repository_url().as_deref(), Some(expected), "url: {url}");
    }
}

/// `npm pkg fix` leaves these URLs as they are because hosted-git-info does
/// not recognise them.
#[test]
fn from_url_rejects_repository_urls_npm_leaves_unchanged() {
    for url in [
        "git://gitlab.com/foo/bar.git",
        "https://example.com/repo.git",
        "http://gist.github.com/abc123",
        "https://gist.github.com/foo/abc123/raw/file.js",
        "https://gist.github.com/abc123/raw/file.js",
        "https://gist.github.com/abc123/raw",
        "https://gist.github.com/",
    ] {
        assert_eq!(HostedGit::from_url(url), None, "url: {url}");
    }
}

#[test]
fn gist_shortcut() {
    let hosted = HostedGit::from_url("gist:abc123#deadbeef").expect("recognised");
    assert_eq!(hosted.host_type, HostedGitType::Gist);
    assert_eq!(hosted.user, "");
    assert_eq!(hosted.project, "abc123");
    assert_eq!(hosted.committish.as_deref(), Some("deadbeef"));
    assert!(hosted.names_repository());
}

#[test]
fn gist_url_with_and_without_owner() {
    for url in ["https://gist.github.com/abc123", "https://gist.github.com/foo/abc123.git"] {
        let hosted = HostedGit::from_url(url).expect("recognised");
        assert_eq!(hosted.host_type, HostedGitType::Gist, "url: {url}");
        assert_eq!(hosted.project, "abc123", "url: {url}");
    }
    assert_eq!(HostedGit::from_url("https://gist.github.com/foo/abc123").unwrap().user, "foo");
}

/// Every gist form omits the owner, as upstream's gist templates do.
#[test]
fn gist_url_forms_omit_the_owner() {
    let hosted = HostedGit::from_url("https://gist.github.com/foo/abc123#deadbeef").unwrap();
    let opts = HostedOpts::default();
    assert_eq!(hosted.shortcut(opts), "gist:abc123#deadbeef");
    assert_eq!(hosted.https(opts).unwrap(), "git+https://gist.github.com/abc123.git#deadbeef");
    assert_eq!(hosted.ssh(opts).unwrap(), "git@gist.github.com:abc123.git#deadbeef");
    assert_eq!(hosted.sshurl(opts).unwrap(), "git+ssh://git@gist.github.com/abc123.git#deadbeef");
    assert_eq!(
        hosted.tarball(opts).unwrap(),
        "https://codeload.github.com/gist/abc123/tar.gz/deadbeef",
    );
    assert_eq!(hosted.synthesized_package_name(), "abc123");
}

#[test]
fn gist_package_docs_url() {
    assert_eq!(
        HostedGit::package_docs_url("gist:abc123").as_deref(),
        Some("https://gist.github.com/abc123"),
    );
    assert_eq!(
        HostedGit::package_docs_url("https://gist.github.com/foo/abc123#deadbeef").as_deref(),
        Some("https://gist.github.com/abc123/deadbeef"),
    );
}

/// The gist forms keep URL-embedded credentials so `git ls-remote` can
/// still reach a private gist. The `repository` field `npm pkg fix` writes
/// drops them, as upstream's gist templates do.
#[test]
fn gist_https_keeps_credentials() {
    let hosted = HostedGit::from_url("https://user:token@gist.github.com/abc123.git").unwrap();
    assert_eq!(
        hosted
            .https(HostedOpts::default())
            .unwrap(),
        "git+https://user:token@gist.github.com/abc123.git",
    );
    assert_eq!(
        hosted.normalized_repository_url().unwrap(),
        "git+https://gist.github.com/abc123.git",
    );
}
