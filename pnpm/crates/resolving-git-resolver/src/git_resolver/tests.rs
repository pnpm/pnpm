use super::{GitProbe, GitResolver, ProbeFuture};
use crate::resolve_ref::{GitCommandRunner, GitRunError};
use miette::Diagnostic;
use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_resolver_base::{
    GitResolveError, ResolveOptions, ResolveResult, Resolver, WantedDependency,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

struct FakeProbe {
    /// Whether the anonymous HEAD of the archive URL succeeds — the
    /// public/private axis of the repo under test.
    archive_ok: bool,
    calls: Mutex<Vec<String>>,
}

impl FakeProbe {
    fn new(archive_ok: bool) -> Self {
        Self { archive_ok, calls: Mutex::new(Vec::new()) }
    }
}

impl GitProbe for FakeProbe {
    fn anonymous_head_ok<'a>(&'a self, url: &'a str) -> ProbeFuture<'a> {
        self.calls.lock().unwrap().push(url.to_string());
        let ok = self.archive_ok;
        Box::pin(async move { ok })
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

/// Stands in for a git that cannot reach the remote at all — a machine
/// without the host's CA certificates, without an SSH key, offline.
struct UnreachableRunner {
    stderr: String,
}

impl GitCommandRunner for UnreachableRunner {
    fn ls_remote<'a>(
        &'a self,
        _repo: &'a str,
        _ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        let message = self.stderr.clone();
        Box::pin(async move { Err(GitRunError { message }) })
    }
}

async fn resolve_unreachable(bare_specifier: &str, stderr: &str) -> GitResolveError {
    let resolver = GitResolver::new(
        Arc::new(FakeProbe::new(true)),
        Arc::new(UnreachableRunner { stderr: stderr.to_string() }),
    );
    let wanted = WantedDependency {
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("unreachable remote");
    *err.downcast::<GitResolveError>().expect("the resolver's own diagnostic, boxed outermost")
}

fn runner(stdout: &str) -> FakeRunner {
    FakeRunner { stdout: stdout.to_string(), calls: Mutex::new(Vec::new()) }
}

/// The fakes ride back with the result because which remote
/// `ls-remote` hits and which archive URL gets probed *are* the
/// behavior under test.
async fn resolve_with(
    archive_ok: bool,
    stdout: &str,
    bare_specifier: &str,
) -> (ResolveResult, Arc<FakeRunner>, Arc<FakeProbe>) {
    let runner = Arc::new(runner(stdout));
    let probe = Arc::new(FakeProbe::new(archive_ok));
    let resolver = GitResolver::new(Arc::clone(&probe), Arc::clone(&runner));
    let wanted = WantedDependency {
        alias: None,
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claimed");
    (result, runner, probe)
}

#[tokio::test]
async fn declines_non_git_specifier() {
    let resolver = GitResolver::new(Arc::new(FakeProbe::new(true)), Arc::new(runner("")));
    let wanted = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("1.2.3".to_string()),
        ..WantedDependency::default()
    };
    assert!(resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().is_none());
}

#[tokio::test]
async fn github_shortcut_full_commit_returns_tarball() {
    const COMMIT: &str = "163360a8d3ae6bee9524541043197ff356f8ed99";
    let (result, runner, probe) =
        resolve_with(true, "", &format!("zkochan/is-negative#{COMMIT}")).await;
    assert_eq!(result.resolved_via, "git-repository");
    match result.resolution {
        LockfileResolution::Tarball(t) => {
            assert_eq!(
                t.tarball,
                format!("https://codeload.github.com/zkochan/is-negative/tar.gz/{COMMIT}"),
            );
            assert_eq!(t.git_hosted, Some(true));
            assert!(t.path.is_none());
        }
        other => panic!("expected Tarball, got {other:?}"),
    }
    assert_eq!(
        result.id.as_str(),
        format!("https://codeload.github.com/zkochan/is-negative/tar.gz/{COMMIT}"),
    );
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some(format!("github:zkochan/is-negative#{COMMIT}").as_str()),
    );
    assert!(runner.calls.lock().unwrap().is_empty(), "a full sha needs no ls-remote");
    assert_eq!(
        probe.calls.lock().unwrap().as_slice(),
        [format!("https://codeload.github.com/zkochan/is-negative/tar.gz/{COMMIT}")],
        "the probe must test the exact URL that gets recorded",
    );
}

#[tokio::test]
async fn archive_probe_failure_records_git_over_https() {
    const COMMIT: &str = "0000000000000000000000000000000000000000";
    let (result, runner, probe) =
        resolve_with(false, &format!("{COMMIT}\tHEAD\n"), "github:foo/bar").await;

    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("github:foo/bar"));
    match &result.resolution {
        LockfileResolution::Git(git) => {
            assert_eq!(git.repo, "https://github.com/foo/bar.git");
            assert_eq!(git.commit, COMMIT);
            assert_eq!(git.path, None);
        }
        other => panic!("expected Git, got {other:?}"),
    }
    assert_eq!(result.id.as_str(), format!("git+https://github.com/foo/bar.git#{COMMIT}"));
    assert_eq!(
        runner.calls.lock().unwrap().as_slice(),
        [("https://github.com/foo/bar.git".to_string(), Some("HEAD".to_string()))],
    );
    assert_eq!(
        probe.calls.lock().unwrap().as_slice(),
        [format!("https://codeload.github.com/foo/bar/tar.gz/{COMMIT}")],
    );
}

#[tokio::test]
async fn hosted_ssh_input_resolves_through_the_https_identity() {
    const COMMIT: &str = "1234567890123456789012345678901234567890";
    let (result, runner, _probe) =
        resolve_with(true, &format!("{COMMIT}\tHEAD\n"), "git+ssh://git@github.com/foo/bar.git")
            .await;

    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("github:foo/bar"));
    match &result.resolution {
        LockfileResolution::Tarball(t) => {
            assert_eq!(t.tarball, format!("https://codeload.github.com/foo/bar/tar.gz/{COMMIT}"));
        }
        other => panic!("expected Tarball, got {other:?}"),
    }
    assert_eq!(
        runner.calls.lock().unwrap().as_slice(),
        [("https://github.com/foo/bar.git".to_string(), Some("HEAD".to_string()))],
        "ls-remote must run against the canonical HTTPS URL, not the SSH form",
    );
}

#[tokio::test]
async fn unknown_host_ssh_url_stays_a_git_resolution() {
    let stdout = "abcdef1234567890123456789012345678901234\tHEAD\n";
    let (result, _runner, probe) =
        resolve_with(true, stdout, "git+ssh://git@example.com/org/repo.git#abcdef12").await;
    match result.resolution {
        LockfileResolution::Git(g) => {
            assert_eq!(g.repo, "ssh://git@example.com/org/repo.git");
            assert_eq!(g.commit, "abcdef1234567890123456789012345678901234");
            assert!(g.path.is_none());
        }
        other => panic!("expected Git, got {other:?}"),
    }
    assert!(result.id.as_str().starts_with("git+ssh://git@example.com/org/repo.git#"));
    assert!(probe.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn path_suffix_appended_to_id_and_resolution() {
    let stdout = "1111111111111111111111111111111111111111\tHEAD\n";
    let (result, _runner, _probe) = resolve_with(
        true,
        stdout,
        "github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app",
    )
    .await;
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
    let (result, runner, _probe) = resolve_with(
        true,
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

#[tokio::test]
async fn credentialed_https_url_keeps_the_authenticated_url() {
    const COMMIT: &str = "0000000000000000000000000000000000000000";
    const AUTH_URL: &str =
        "https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git";
    let (result, runner, probe) = resolve_with(
        true,
        &format!("{COMMIT}\tHEAD\n"),
        "git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git",
    )
    .await;

    assert_eq!(result.resolved_via, "git-repository");
    assert_eq!(
        result.normalized_bare_specifier.as_deref(),
        Some(format!("git+{AUTH_URL}").as_str()),
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
    assert!(probe.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unreachable_remote_names_the_dependency_and_how_to_substitute_the_transport() {
    let err = resolve_unreachable(
        "zkochan/is-negative#next",
        "fatal: unable to access 'https://github.com/zkochan/is-negative.git/': SSL certificate problem",
    )
    .await;

    assert_eq!(err.code().expect("code").to_string(), "ERR_PNPM_GIT_RESOLVE_FAILED");
    assert_eq!(
        err.to_string(),
        r#"Failed to resolve git dependency "zkochan/is-negative#next": git ls-remote failed: fatal: unable to access 'https://github.com/zkochan/is-negative.git/': SSL certificate problem"#,
    );
    let help = err.help().expect("help").to_string();
    assert!(
        help.contains(
            r#"git config --global url."git@github.com:".insteadOf "https://github.com/""#
        ),
        "{help}",
    );
}

// A known host's SSH URL is an identity that finalises to HTTPS (see
// `parse_bare_specifier`), so the hint applies there too. Only an unknown
// host's URL keeps the transport the user wrote.
#[tokio::test]
async fn unreachable_ssh_remote_carries_no_transport_substitution_hint() {
    let err = resolve_unreachable(
        "git+ssh://git@example.com/foo/bar.git",
        "git@example.com: Permission denied (publickey).",
    )
    .await;

    assert!(err.help().is_none());
}
