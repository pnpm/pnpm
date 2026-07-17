//! [`GitResolver`] wires the parser, the host probe, and the
//! ls-remote runner into a single [`Resolver`] the dispatcher can
//! compose into the default-resolver chain.

use std::sync::Arc;

use pacquet_git_fetcher::{GitManifestQuery, read_git_manifest};
use pacquet_lockfile::{GitResolution, LockfileResolution, TarballResolution};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_reporter::SilentReporter;
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use pacquet_store_dir::{StoreDir, StoreIndexWriter};
use pacquet_tarball::{FetchTarballForResolution, RetryOpts};

use crate::{
    create_git_hosted_pkg_id::create_git_hosted_pkg_id,
    hosted_git::HostedOpts,
    parse_bare_specifier::{GitProbe, HostedPackageSpec, parse_bare_specifier},
    resolve_ref::{GitCommandRunner, resolve_ref},
};

/// Store/network handles [`GitResolver`] needs to read a git dep's
/// identity out of the package itself during resolution.
///
/// A git dep's specifier names a repo, not a package, so its name —
/// and, for a host archive, its integrity — live only in the package's
/// own `package.json`. pacquet builds the lockfile before the
/// install/fetch pass runs, so they have to be read here. Mirrors the
/// tarball resolver's remote-tarball fetch, which fills the same fields
/// for the same reason.
///
/// Two shapes, by resolution:
///
/// - a git *host* (`github:` / `gitlab:` / `bitbucket:`) serves an
///   archive, which is downloaded and hashed;
/// - any other repo (ssh, self-hosted, `file:`) has no archive
///   endpoint, so a throwaway checkout is the cheapest read.
///
/// Either way this stops at the manifest: `prepare` / `prepublish` and
/// packlist filtering stay in the install pass, so no package script
/// runs during resolution. The install pass re-fetches to run them —
/// unlike a registry tarball, a git-hosted one can't hand its
/// extraction over through `MemCache` (only `Registry` resolutions read
/// it) — so a git dep costs one extra fetch per install.
pub struct GitFetchContext {
    pub http_client: Arc<ThrottledClient>,
    pub store_dir: &'static StoreDir,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    pub auth_headers: Arc<AuthHeaders>,
    pub retry_opts: RetryOpts,
    /// Hosts that opt into `git init` + `git fetch --depth 1` instead
    /// of a full clone, for the repos with no archive endpoint. Mirrors
    /// `Config::git_shallow_hosts`.
    pub git_shallow_hosts: Vec<String>,
}

/// Git resolver entry point. Holds the production network / git
/// runners shared across every per-dep `resolve()` call; tests
/// construct one with fake runners.
///
/// `Arc` so the resolver can be cloned into the default-resolver
/// chain without forcing the runners (whose ownership lives on the
/// install dispatcher) into a single owner.
///
/// When `fetch_context` is `Some`, the package is read during
/// resolution to fill `manifest` (and `integrity`, for a host archive)
/// — see [`GitFetchContext`]. `None` (unit tests, and the resolve-only
/// NAPI entry point) keeps the manifest-less shape.
pub struct GitResolver<Probe: GitProbe + 'static, Runner: GitCommandRunner + 'static> {
    probe: Arc<Probe>,
    runner: Arc<Runner>,
    fetch_context: Option<GitFetchContext>,
}

impl<Probe: GitProbe + 'static, Runner: GitCommandRunner + 'static> GitResolver<Probe, Runner> {
    pub fn new(probe: Arc<Probe>, runner: Arc<Runner>) -> Self {
        Self { probe, runner, fetch_context: None }
    }

    /// Attach the store/network handles that let resolution read the
    /// package's name from its `package.json`.
    #[must_use]
    pub fn with_fetch_context(mut self, fetch_context: GitFetchContext) -> Self {
        self.fetch_context = Some(fetch_context);
        self
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
        let mut result =
            build_resolve_result(spec, self.runner.as_ref(), wanted_dependency.alias.as_deref())
                .await?;
        self.read_package_metadata(&mut result).await?;
        Ok(Some(result))
    }

    /// Fill `manifest` — and, for an archive, the resolution's
    /// `integrity` — from the package the git dep points at. No-op
    /// without a fetch context (unit tests / resolve-only callers).
    async fn read_package_metadata(&self, result: &mut ResolveResult) -> Result<(), ResolveError> {
        let Some(ctx) = self.fetch_context.as_ref() else { return Ok(()) };
        match &result.resolution {
            LockfileResolution::Tarball(tarball) => {
                let tarball_url = tarball.tarball.clone();
                // `#path:/packages/foo` points at one directory of the
                // repo; the archive spans the whole repo, so its root
                // `package.json` is the repo's, not this package's.
                let manifest_subdir = tarball.path.clone();

                // Silent reporter: the install pass owns the
                // `resolved → found_in_store → imported` event ordering.
                let resolved = FetchTarballForResolution {
                    http_client: &ctx.http_client,
                    store_dir: ctx.store_dir,
                    store_index_writer: ctx.store_index_writer.clone(),
                    package_url: &tarball_url,
                    // A git host's archive URL is the package's only
                    // identifier at this point — its name is what this
                    // fetch is here to learn — and such archives carry
                    // no scoped-registry auth.
                    package_id: &tarball_url,
                    auth_headers: &ctx.auth_headers,
                    retry_opts: ctx.retry_opts,
                    manifest_subdir: manifest_subdir.as_deref(),
                }
                .run::<SilentReporter>(None)
                .await
                .map_err(|err| Box::new(err) as ResolveError)?;

                result.manifest = resolved.manifest.map(Arc::new);
                if let LockfileResolution::Tarball(tarball) = &mut result.resolution {
                    // A git host's archive carries no integrity of its
                    // own, and the install pass refuses a tarball
                    // resolution without one
                    // (`tarball_url_and_integrity`). The bytes were
                    // just hashed to extract them, so record that —
                    // same field upstream writes for a git dep.
                    tarball.integrity = Some(resolved.integrity);
                }
            }
            LockfileResolution::Git(git) => {
                // No archive endpoint to read, so the working tree is
                // the only source of the name. A `Git` resolution
                // carries no integrity — it is anchored by its commit —
                // so the manifest is all there is to read.
                let manifest = read_git_manifest(GitManifestQuery {
                    repo: &git.repo,
                    commit: &git.commit,
                    path: git.path.as_deref(),
                    git_shallow_hosts: &ctx.git_shallow_hosts,
                    git_bin: None,
                })
                .await
                .map_err(|err| Box::new(err) as ResolveError)?;
                result.manifest = manifest.map(Arc::new);
            }
            _ => {}
        }
        Ok(())
    }

    /// Companion to [`Self::resolve_impl`].
    ///
    /// Claims every dep the parser recognises, but returns an empty
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

/// Pick between a tarball and a git resolution.
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
