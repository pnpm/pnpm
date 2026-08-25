//! [`GitResolver`] wires the parser, the host probe, and the
//! ls-remote runner into a single [`Resolver`] the dispatcher can
//! compose into the default-resolver chain.

use std::{future::Future, pin::Pin, sync::Arc};

use pnpm_git_fetcher::{GitManifestQuery, read_git_manifest};
use pnpm_lockfile::{GitResolution, LockfileResolution, TarballResolution};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_reporter::SilentReporter;
use pnpm_resolving_resolver_base::{
    GitResolveError, LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture,
    ResolveOptions, ResolveResult, Resolver, WantedDependency,
};
use pnpm_store_dir::{StoreDir, StoreIndexWriter};
use pnpm_tarball::{FetchTarballForResolution, RetryOpts};

use crate::{
    create_git_hosted_pkg_id::create_git_hosted_pkg_id,
    hosted_git::HostedOpts,
    parse_bare_specifier::{HostedPackageSpec, parse_bare_specifier},
    resolve_ref::{GitCommandRunner, GitResolveRefError, resolve_ref},
};

/// Boxed-future return type used by [`GitProbe`]. Same shape as the
/// rest of pacquet's async traits (see `ResolveFuture`).
pub type ProbeFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// Capability seam for the one network check resolution performs: an
/// anonymous HTTP HEAD of a host's archive URL.
///
/// The host-archive (tarball) shape is recorded only when this probe
/// proves the exact URL to be recorded anonymously fetchable, so a
/// recorded archive URL is valid by construction. On probe failure the
/// resolution falls back to `type: git` over the canonical HTTPS URL,
/// which every machine with access to the repo can fetch — a false
/// negative (private repo, host throttling past the retries) costs
/// archive speed on one dependency, never correctness.
pub trait GitProbe: Send + Sync {
    /// `true` when an anonymous HTTP HEAD of `url` returned 2xx.
    /// Implementations retry transient failures (429/5xx/network
    /// errors) so host throttling is not mistaken for a private
    /// repository.
    fn anonymous_head_ok<'a>(&'a self, url: &'a str) -> ProbeFuture<'a>;
}

/// Store/network handles [`GitResolver`] needs to read a git dep's
/// identity out of the package itself during resolution.
///
/// A git dep's specifier names a repo, not a package, so its package name
/// lives only in the package's own `package.json`. For a host archive,
/// integrity is computed from the fetched bytes and stored on the resolution.
/// pacquet builds the lockfile before the install/fetch pass runs, so both
/// fields have to be filled here. Mirrors the tarball resolver's
/// remote-tarball fetch, which fills the same fields for the same reason.
///
/// Two shapes, by resolution:
///
/// - a git *host*'s anonymously fetchable archive (see [`GitProbe`])
///   is downloaded and hashed;
/// - any other repo (unknown host, private hosted repo, `file:`) has
///   no usable archive endpoint, so a throwaway checkout is the
///   cheapest read.
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
        let spec = partial.finalize();
        let mut result = build_resolve_result(
            spec,
            self.probe.as_ref(),
            self.runner.as_ref(),
            wanted_dependency,
        )
        .await?;
        self.read_package_metadata(&mut result).await?;
        Ok(Some(result))
    }

    /// Fill `manifest` from the package the git dep points at and compute an
    /// archive resolution's `integrity` from its fetched bytes. No-op without
    /// a fetch context (unit tests / resolve-only callers).
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
                // the only source of the name, and there is nothing to
                // hash — the commit anchors the content.
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

async fn build_resolve_result<Probe: GitProbe + ?Sized, Runner: GitCommandRunner + ?Sized>(
    spec: HostedPackageSpec,
    probe: &Probe,
    runner: &Runner,
    wanted_dependency: &WantedDependency,
) -> Result<ResolveResult, ResolveError> {
    let ref_for_ls_remote = match spec.git_committish.as_deref() {
        Some(committish) if !committish.is_empty() => committish,
        _ => "HEAD",
    };
    let commit =
        resolve_ref(runner, &spec.fetch_spec, ref_for_ls_remote, spec.git_range.as_deref())
            .await
            .map_err(|err| ref_resolution_error(err, wanted_dependency, &spec.fetch_spec))?;

    let resolution = pick_resolution(&spec, probe, &commit).await;

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
        alias: wanted_dependency.alias.clone(),
        policy_violation: None,
    })
}

/// Box a ref-resolution failure as a [`ResolveError`], naming the dependency
/// when the `git ls-remote` invocation itself failed.
///
/// [`GitResolveError`] has to be the outermost box for the tree walker's
/// downcast to find it, which is what keeps its code and help alive across
/// the type-erased resolver seam.
fn ref_resolution_error(
    err: GitResolveRefError,
    wanted_dependency: &WantedDependency,
    repo: &str,
) -> ResolveError {
    let GitResolveRefError::Runner(ls_remote) = &err else {
        return Box::new(err) as ResolveError;
    };
    let specifier = wanted_dependency.bare_specifier.as_deref().unwrap_or_default();
    Box::new(GitResolveError::new(specifier, repo, &ls_remote.to_string())) as ResolveError
}

/// Pick between a tarball and a git resolution — see [`GitProbe`] for
/// the rule and its fail-safe direction.
async fn pick_resolution<Probe: GitProbe + ?Sized>(
    spec: &HostedPackageSpec,
    probe: &Probe,
    commit: &str,
) -> LockfileResolution {
    if let Some(hosted) = spec.hosted.as_ref() {
        let mut hosted = hosted.clone();
        hosted.committish = Some(commit.to_string());
        if let Some(tarball) = hosted.tarball(HostedOpts::default())
            && probe.anonymous_head_ok(&tarball).await
        {
            return LockfileResolution::Tarball(TarballResolution {
                tarball,
                integrity: None,
                revision: None,
                git_hosted: Some(true),
                path: spec.path.clone(),
            });
        }
    }
    LockfileResolution::Git(GitResolution {
        repo: spec.fetch_spec.clone(),
        commit: commit.to_string(),
        integrity: None,
        path: spec.path.clone(),
    })
}

#[cfg(test)]
mod tests;
