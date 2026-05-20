//! Chain-friendly wrapper that implements
//! [`pacquet_resolving_resolver_base::Resolver`] over the free
//! functions in [`super::local_resolver`].
//!
//! Equivalent to upstream's
//! [`resolveSchemeOrPath`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/default-resolver/src/index.ts#L97-L173)
//! step inside `createResolver`: try the scheme-prefix interpretation
//! first; fall through to the path-shape interpretation; defer
//! (`Ok(None)`) when neither claims.

use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, UpdateBehavior, WantedDependency,
};

use crate::local_resolver::{
    LocalResolverContext, LocalResolverOptions, LocalResolverUpdate, resolve_from_local_path,
    resolve_from_local_scheme, resolve_latest_from_local,
};
use crate::parse_bare_specifier::WantedLocalDependency;

/// `Resolver`-trait wrapper that the default-resolver chain consumes.
/// Holds the install-scoped [`LocalResolverContext`] (just
/// `preserveAbsolutePaths` today); the per-resolve `project_dir` /
/// `lockfile_dir` / `update` come from [`ResolveOptions`].
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalResolver {
    pub ctx: LocalResolverContext,
}

impl LocalResolver {
    pub fn new(ctx: LocalResolverContext) -> Self {
        Self { ctx }
    }
}

impl Resolver for LocalResolver {
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
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        let info = resolve_latest_from_local(query);
        Box::pin(async move { Ok(info) })
    }
}

impl LocalResolver {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(bare) = wanted_dependency.bare_specifier.clone() else {
            return Ok(None);
        };
        let wd = WantedLocalDependency {
            bare_specifier: bare,
            injected: wanted_dependency.injected.unwrap_or(false),
        };
        let local_opts = LocalResolverOptions {
            project_dir: opts.project_dir.clone(),
            lockfile_dir: Some(opts.lockfile_dir.clone()),
            current_pkg: None,
            update: match opts.update {
                UpdateBehavior::Off => LocalResolverUpdate::Off,
                UpdateBehavior::Compatible | UpdateBehavior::Latest => LocalResolverUpdate::On,
            },
        };

        if let Some(result) = resolve_from_local_scheme(&self.ctx, &wd, &local_opts)
            .await
            .map_err(|err| Box::new(err) as ResolveError)?
        {
            return Ok(Some(into_chain_result(result, wanted_dependency)));
        }

        if let Some(result) = resolve_from_local_path(&self.ctx, &wd, &local_opts)
            .await
            .map_err(|err| Box::new(err) as ResolveError)?
        {
            return Ok(Some(into_chain_result(result, wanted_dependency)));
        }

        Ok(None)
    }
}

/// Thread the alias from the [`WantedDependency`] onto the chain
/// result so the install layer can address the resolved package by
/// the manifest key. Mirrors upstream's
/// [`alias: wantedDep.alias`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/default-resolver/src/index.ts#L123)
/// thread.
fn into_chain_result(
    result: crate::local_resolver::LocalResolveResult,
    wanted_dependency: &WantedDependency,
) -> ResolveResult {
    let mut chain: ResolveResult = result.into();
    chain.alias = wanted_dependency.alias.clone();
    chain
}
