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
mod tests;
