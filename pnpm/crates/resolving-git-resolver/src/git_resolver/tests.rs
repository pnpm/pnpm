use super::{GitProbe, GitResolver};
use crate::{
    parse_bare_specifier::ProbeFuture,
    resolve_ref::{GitCommandRunner, GitRunError},
};
use pacquet_lockfile::LockfileResolution;
use pacquet_resolving_resolver_base::{ResolveOptions, ResolveResult, Resolver, WantedDependency};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

struct FakeProbe {
    head_ok: bool,
    ls_ok: bool,
    /// When non-empty, `ls-remote --exit-code` succeeds only for these
    /// URLs and `ls_ok` is ignored. Models a repo that is reachable
    /// over exactly one of the candidate transports — which is what
    /// decides the fetch spec for a private repo.
    ls_ok_only_for: Vec<String>,
}

impl FakeProbe {
    /// A public repo: both probes succeed for every URL.
    fn public() -> Self {
        Self { head_ok: true, ls_ok: true, ls_ok_only_for: Vec::new() }
    }

    /// A private repo — the HTTPS HEAD that detects a public repo comes
    /// back non-2xx — reachable only over `url`. Upstream's
    /// `mockFetchAsPrivate` plus a `git` mock that throws for every
    /// other remote.
    fn private_reachable_over(url: &str) -> Self {
        Self { head_ok: false, ls_ok: false, ls_ok_only_for: vec![url.to_string()] }
    }
}

impl GitProbe for FakeProbe {
    fn https_head_ok<'a>(&'a self, _url: &'a str) -> ProbeFuture<'a> {
        let enabled = self.head_ok;
        Box::pin(async move { enabled })
    }
    fn ls_remote_exit_code<'a>(&'a self, repo: &'a str) -> ProbeFuture<'a> {
        let enabled = if self.ls_ok_only_for.is_empty() {
            self.ls_ok
        } else {
            self.ls_ok_only_for.iter().any(|allowed| allowed == repo)
        };
        Box::pin(async move { enabled })
    }
}

struct FakeRunner {
    stdout: String,
    calls: Mutex<Vec<(String, Option<String>)>>,
}
impl GitCommandRunner for FakeRunner {
    fn ls_remote<'a>(
        &'a self,
        repo: &'a str,
        ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        self.calls.lock().unwrap().push((repo.to_string(), ref_.map(str::to_string)));
        let stdout = self.stdout.clone();
        Box::pin(async move { Ok(stdout) })
    }
}

fn resolver(head_ok: bool, ls_ok: bool, stdout: &str) -> GitResolver<FakeProbe, FakeRunner> {
    GitResolver::new(
        Arc::new(FakeProbe { head_ok, ls_ok, ls_ok_only_for: Vec::new() }),
        Arc::new(runner(stdout)),
    )
}

fn runner(stdout: &str) -> FakeRunner {
    FakeRunner { stdout: stdout.to_string(), calls: Mutex::new(Vec::new()) }
}

/// Resolve `bare_specifier`, returning the result alongside the runner
/// so a test can assert which remote `ls-remote` was pointed at — for a
/// private repo that choice *is* the behavior under test.
async fn resolve_with(
    probe: FakeProbe,
    stdout: &str,
    bare_specifier: &str,
) -> (ResolveResult, Arc<FakeRunner>) {
    let runner = Arc::new(runner(stdout));
    let resolver = GitResolver::new(Arc::new(probe), Arc::clone(&runner));
    let wanted = WantedDependency {
        alias: None,
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claimed");
    (result, runner)
}

#[tokio::test]
async fn declines_non_git_specifier() {
    let resolver = resolver(true, true, "");
    let wanted = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("1.2.3".to_string()),
        ..WantedDependency::default()
    };
    assert!(resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().is_none());
}

#[tokio::test]
async fn github_shortcut_full_commit_returns_tarball() {
    let resolver = resolver(true, true, "");
    let wanted = WantedDependency {
        alias: None,
        bare_specifier: Some(
            "zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99".to_string(),
        ),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claimed");
    assert_eq!(result.resolved_via, "git-repository");
    match result.resolution {
        LockfileResolution::Tarball(t) => {
            assert_eq!(
                t.tarball,
                "https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99",
            );
            assert_eq!(t.git_hosted, Some(true));
            assert!(t.path.is_none());
        }
        other => panic!("expected Tarball, got {other:?}"),
    }
    assert_eq!(
        result.id.as_str(),
        "https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99",
    );
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some("github:zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99"),
    );
}

#[tokio::test]
async fn ssh_url_falls_back_to_git_resolution() {
    let stdout = "abcdef1234567890123456789012345678901234\tHEAD\n";
    let resolver = resolver(false, true, stdout);
    let wanted = WantedDependency {
        alias: None,
        bare_specifier: Some("git+ssh://git@example.com/org/repo.git#abcdef12".to_string()),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claimed");
    match result.resolution {
        LockfileResolution::Git(g) => {
            assert_eq!(g.repo, "ssh://git@example.com/org/repo.git");
            assert_eq!(g.commit, "abcdef1234567890123456789012345678901234");
            assert!(g.path.is_none());
        }
        other => panic!("expected Git, got {other:?}"),
    }
    assert!(result.id.as_str().starts_with("git+ssh://git@example.com/org/repo.git#"));
}

#[tokio::test]
async fn path_suffix_appended_to_id_and_resolution() {
    let stdout = "1111111111111111111111111111111111111111\tHEAD\n";
    let resolver = resolver(true, true, stdout);
    let wanted = WantedDependency {
        alias: None,
        bare_specifier: Some(
            "github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app".to_string(),
        ),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claimed");
    match result.resolution {
        LockfileResolution::Tarball(t) => {
            assert_eq!(t.path.as_deref(), Some("/packages/simple-react-app"));
            assert!(t.tarball.ends_with("/tar.gz/1111111111111111111111111111111111111111"));
        }
        other => panic!("expected Tarball, got {other:?}"),
    }
    assert!(result.id.as_str().ends_with("#path:/packages/simple-react-app"));
}

/// TS: `resolveFromGit() with both sub folder and branch`
/// (`resolving/git-resolver/test/index.ts:211`).
///
/// `#beta&path:/packages/simple-react-app` carries a branch *and* a
/// subdirectory in one fragment: the branch decides the commit the
/// archive URL pins, while the path rides along into the resolution and
/// the id, so two subdirectories of one repo stay distinct packages.
#[tokio::test]
async fn sub_folder_and_branch_resolve_to_a_tarball_carrying_the_path() {
    const BETA_COMMIT: &str = "777e8a3e78cc89bbf41fb3fd9f6cf922d5463313";
    let (result, runner) = resolve_with(
        FakeProbe::public(),
        &format!("{BETA_COMMIT}\trefs/heads/beta\n"),
        "github:RexSkz/test-git-subfolder-fetch.git#beta&path:/packages/simple-react-app",
    )
    .await;

    assert_eq!(result.resolved_via, "git-repository");
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some("github:RexSkz/test-git-subfolder-fetch#beta&path:/packages/simple-react-app"),
    );
    match &result.resolution {
        LockfileResolution::Tarball(tarball) => {
            assert_eq!(
                tarball.tarball,
                format!(
                    "https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/{BETA_COMMIT}"
                ),
            );
            assert_eq!(tarball.path.as_deref(), Some("/packages/simple-react-app"));
            assert_eq!(tarball.git_hosted, Some(true));
        }
        other => panic!("expected Tarball, got {other:?}"),
    }
    assert_eq!(
        result.id.as_str(),
        format!(
            "https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/{BETA_COMMIT}#path:/packages/simple-react-app"
        ),
    );
    assert_eq!(
        runner.calls.lock().unwrap().as_slice(),
        [(
            "https://github.com/RexSkz/test-git-subfolder-fetch.git".to_string(),
            Some("beta".to_string())
        )],
        "the branch, not HEAD, is what ls-remote is asked to resolve",
    );
}

/// TS: `resolve a private repository using the HTTPS protocol without
/// auth token` (`resolving/git-resolver/test/index.ts:482`).
///
/// The HTTPS HEAD that detects a public repo fails and the anonymous
/// HTTPS remote is unreachable, so the only transport left is SSH —
/// and an SSH fetch spec must not resolve to the host's public archive
/// URL.
#[tokio::test]
async fn private_https_repo_without_auth_falls_back_to_the_ssh_url() {
    const SSH_URL: &str = "git+ssh://git@github.com/foo/bar.git";
    const COMMIT: &str = "0000000000000000000000000000000000000000";
    let (result, runner) = resolve_with(
        FakeProbe::private_reachable_over(SSH_URL),
        &format!("{COMMIT}\tHEAD\n"),
        "git+https://github.com/foo/bar.git",
    )
    .await;

    assert_eq!(result.resolved_via, "git-repository");
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("github:foo/bar"));
    match &result.resolution {
        LockfileResolution::Git(git) => {
            assert_eq!(git.repo, SSH_URL);
            assert_eq!(git.commit, COMMIT);
            assert_eq!(git.path, None);
        }
        other => panic!("expected Git, got {other:?}"),
    }
    assert_eq!(result.id.as_str(), format!("{SSH_URL}#{COMMIT}"));
    assert_eq!(
        runner.calls.lock().unwrap().as_slice(),
        [(SSH_URL.to_string(), Some("HEAD".to_string()))],
    );
}

/// TS: `resolve a private repository using the HTTPS protocol and an
/// auth token` (`resolving/git-resolver/test/index.ts:526`).
///
/// The credentials in the URL are what make the repo reachable, and a
/// host's archive endpoint does not carry them — so this must stay a
/// `type: git` resolution against the authenticated remote rather than
/// collapsing to a `codeload` URL nothing could fetch.
#[tokio::test]
async fn private_https_repo_with_an_auth_token_keeps_the_authenticated_url() {
    const COMMIT: &str = "0000000000000000000000000000000000000000";
    const AUTH_URL: &str =
        "https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git";
    let (result, runner) = resolve_with(
        FakeProbe::private_reachable_over(AUTH_URL),
        &format!("{COMMIT}\tHEAD\n"),
        "git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git",
    )
    .await;

    assert_eq!(result.resolved_via, "git-repository");
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some(format!("git+{AUTH_URL}").as_str())
    );
    match &result.resolution {
        LockfileResolution::Git(git) => {
            assert_eq!(git.repo, AUTH_URL);
            assert_eq!(git.commit, COMMIT);
            assert_eq!(git.path, None);
        }
        other => panic!("expected Git, got {other:?}"),
    }
    assert_eq!(result.id.as_str(), format!("git+{AUTH_URL}#{COMMIT}"));
    assert_eq!(
        runner.calls.lock().unwrap().as_slice(),
        [(AUTH_URL.to_string(), Some("HEAD".to_string()))],
    );
}
