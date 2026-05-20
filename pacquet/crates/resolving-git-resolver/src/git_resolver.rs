//! Pacquet port of pnpm's
//! [`createGitResolver`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/index.ts#L25-L102).
//!
//! [`GitResolver`] wires the parser, the host probe, and the
//! ls-remote runner into a single [`Resolver`] the dispatcher can
//! compose into the default-resolver chain.

use std::sync::Arc;

use pacquet_lockfile::{GitResolution, LockfileResolution, TarballResolution};
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};

use crate::{
    create_git_hosted_pkg_id::create_git_hosted_pkg_id,
    hosted_git::HostedOpts,
    parse_bare_specifier::{GitProbe, HostedPackageSpec, parse_bare_specifier},
    resolve_ref::{GitCommandRunner, resolve_ref},
};

/// Git resolver entry point. Holds the production network / git
/// runners shared across every per-dep `resolve()` call; tests
/// construct one with fake runners.
///
/// `Arc` so the resolver can be cloned into the default-resolver
/// chain without forcing the runners (whose ownership lives on the
/// install dispatcher) into a single owner.
pub struct GitResolver<Probe: GitProbe + 'static, Runner: GitCommandRunner + 'static> {
    probe: Arc<Probe>,
    runner: Arc<Runner>,
}

impl<Probe: GitProbe + 'static, Runner: GitCommandRunner + 'static> GitResolver<Probe, Runner> {
    pub fn new(probe: Arc<Probe>, runner: Arc<Runner>) -> Self {
        Self { probe, runner }
    }
}

impl<Probe: GitProbe + 'static, Runner: GitCommandRunner + 'static> Resolver
    for GitResolver<Probe, Runner>
{
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(self.resolve_impl(wanted_dependency, opts))
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(self.resolve_latest_impl(query, opts))
    }
}

impl<Probe: GitProbe + 'static, Runner: GitCommandRunner + 'static> GitResolver<Probe, Runner> {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        _opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(bare) = wanted_dependency.bare_specifier.as_deref() else { return Ok(None) };
        let Some(partial) = parse_bare_specifier(bare) else { return Ok(None) };
        let spec = partial.finalize(self.probe.as_ref()).await;
        let result =
            build_resolve_result(spec, self.runner.as_ref(), wanted_dependency.alias.as_deref())
                .await?;
        Ok(Some(result))
    }

    /// Companion to [`Self::resolve_impl`].
    ///
    /// Mirrors pnpm's
    /// [`resolveLatestFromGit`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/index.ts#L108-L114):
    /// claim every dep the parser recognises, but return an empty
    /// [`LatestInfo`] (git has no uniform "latest" notion — a host's
    /// tag list would be the closest proxy and the protocols disagree).
    async fn resolve_latest_impl(
        &self,
        query: &LatestQuery,
        _opts: &ResolveOptions,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        let Some(bare) = query.wanted_dependency.bare_specifier.as_deref() else {
            return Ok(None);
        };
        if parse_bare_specifier(bare).is_none() {
            return Ok(None);
        }
        Ok(Some(LatestInfo::default()))
    }
}

async fn build_resolve_result<Runner: GitCommandRunner + ?Sized>(
    spec: HostedPackageSpec,
    runner: &Runner,
    alias: Option<&str>,
) -> Result<ResolveResult, ResolveError> {
    let ref_for_ls_remote = match spec.git_committish.as_deref() {
        Some(committish) if !committish.is_empty() => committish,
        _ => "HEAD",
    };
    let commit =
        resolve_ref(runner, &spec.fetch_spec, ref_for_ls_remote, spec.git_range.as_deref())
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;

    let resolution = pick_resolution(&spec, &commit);

    let id_string = match &resolution {
        LockfileResolution::Tarball(t) => {
            let mut id = t.tarball.clone();
            if let Some(path) = &t.path {
                id.push_str("#path:");
                id.push_str(path);
            }
            id
        }
        LockfileResolution::Git(g) => {
            create_git_hosted_pkg_id(&g.repo, &g.commit, g.path.as_deref())
        }
        _ => unreachable!("pick_resolution returns Tarball or Git only"),
    };

    Ok(ResolveResult {
        id: id_string.into(),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: None,
        resolution,
        resolved_via: "git-repository".to_string(),
        normalized_bare_specifier: Some(spec.normalized_bare_specifier),
        alias: alias.map(str::to_string),
        policy_violation: None,
    })
}

/// Pick between a tarball and a git resolution. Mirrors the
/// `resolution = …` branch in upstream's
/// [`resolveFromGit`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/index.ts#L60-L83).
fn pick_resolution(spec: &HostedPackageSpec, commit: &str) -> LockfileResolution {
    if let Some(hosted) = spec.hosted.as_ref()
        && !is_ssh(&spec.fetch_spec)
    {
        // For hosted, non-ssh repos: produce a tarball URL the
        // git-hosted tarball fetcher can pick up. Build it from a
        // clone of the hosted struct with the resolved committish
        // pinned in.
        let mut hosted = hosted.clone();
        hosted.committish = Some(commit.to_string());
        if let Some(tarball) = hosted.tarball(HostedOpts::default()) {
            return LockfileResolution::Tarball(TarballResolution {
                tarball,
                integrity: None,
                git_hosted: Some(true),
                path: spec.path.clone(),
            });
        }
    }
    LockfileResolution::Git(GitResolution {
        repo: spec.fetch_spec.clone(),
        commit: commit.to_string(),
        path: spec.path.clone(),
    })
}

fn is_ssh(spec: &str) -> bool {
    spec.starts_with("git+ssh://") || spec.starts_with("git@")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::{future::Future, pin::Pin};

    use pacquet_lockfile::LockfileResolution;
    use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};

    use super::{GitProbe, GitResolver};
    use crate::parse_bare_specifier::ProbeFuture;
    use crate::resolve_ref::{GitCommandRunner, GitRunError};

    struct FakeProbe {
        head_ok: bool,
        ls_ok: bool,
    }
    impl GitProbe for FakeProbe {
        fn https_head_ok<'a>(&'a self, _url: &'a str) -> ProbeFuture<'a> {
            let v = self.head_ok;
            Box::pin(async move { v })
        }
        fn ls_remote_exit_code<'a>(&'a self, _repo: &'a str) -> ProbeFuture<'a> {
            let v = self.ls_ok;
            Box::pin(async move { v })
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
        // head_ok=false → first https branch fails; ls_ok=true → ssh branch wins.
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
        // id is git+ssh:// shaped via create_git_hosted_pkg_id.
        assert!(result.id.as_str().starts_with("git+ssh://git@example.com/org/repo.git#"));
    }

    #[tokio::test]
    async fn path_suffix_appended_to_id_and_resolution() {
        let stdout = "1111111111111111111111111111111111111111\tHEAD\n";
        let resolver = resolver(true, true, stdout);
        let wanted = WantedDependency {
            alias: None,
            bare_specifier: Some(
                "github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app"
                    .to_string(),
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
}
