use super::{GitProbe, GitResolver};
use crate::{
    parse_bare_specifier::ProbeFuture,
    resolve_ref::{GitCommandRunner, GitRunError},
};
use pacquet_lockfile::LockfileResolution;
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

struct FakeProbe {
    head_ok: bool,
    ls_ok: bool,
}
impl GitProbe for FakeProbe {
    fn https_head_ok<'a>(&'a self, _url: &'a str) -> ProbeFuture<'a> {
        let enabled = self.head_ok;
        Box::pin(async move { enabled })
    }
    fn ls_remote_exit_code<'a>(&'a self, _repo: &'a str) -> ProbeFuture<'a> {
        let enabled = self.ls_ok;
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
        Arc::new(FakeProbe { head_ok, ls_ok }),
        Arc::new(FakeRunner { stdout: stdout.to_string(), calls: Mutex::new(Vec::new()) }),
    )
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
