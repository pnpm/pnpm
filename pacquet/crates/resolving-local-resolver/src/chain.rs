//! Chain-friendly wrappers that implement
//! [`pacquet_resolving_resolver_base::Resolver`] over the free
//! functions in [`super::local_resolver`].
//!
//! Upstream's chain at
//! [`createResolver`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/default-resolver/src/index.ts#L97-L173)
//! interleaves the local-scheme step ahead of the runtime / named-
//! registry resolvers and runs the local-path step last. Pacquet
//! mirrors that split with two separate [`Resolver`] impls:
//! [`LocalSchemeResolver`] (claims `link:` / `file:` / `workspace:`)
//! and [`LocalPathResolver`] (claims bare path-shape specifiers like
//! `./foo` or `foo.tgz`). [`LocalResolver`] is the combined form
//! kept for tests and one-off chains that don't need the split.

use crate::{
    local_resolver::{
        LocalResolverContext, LocalResolverOptions, LocalResolverUpdate, resolve_from_local_path,
        resolve_from_local_scheme, resolve_latest_from_local,
    },
    parse_bare_specifier::WantedLocalDependency,
};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, UpdateBehavior, WantedDependency,
};

/// `Resolver` for the local-scheme branch (`link:` / `file:` /
/// `workspace:`). Sits between the tarball resolver and the runtime
/// / named-registry resolvers in the chain, mirroring
/// [`_resolveFromLocalScheme`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/default-resolver/src/index.ts#L135).
///
/// `resolve_latest` routes through
/// [`resolve_latest_from_local`]
/// so a `link:` / `file:` / `workspace:` spec stops here instead of
/// falling through into a user-configured named-registry alias of
/// the same name.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalSchemeResolver {
    pub ctx: LocalResolverContext,
}

impl LocalSchemeResolver {
    #[must_use]
    pub fn new(ctx: LocalResolverContext) -> Self {
        Self { ctx }
    }
}

impl Resolver for LocalSchemeResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let Some(wd) = wanted_local(wanted_dependency) else {
                return Ok(None);
            };
            let local_opts = local_options(opts);
            let Some(result) = resolve_from_local_scheme(&self.ctx, &wd, &local_opts)
                .await
                .map_err(|err| Box::new(err) as ResolveError)?
            else {
                return Ok(None);
            };
            Ok(Some(into_chain_result(result, wanted_dependency)))
        })
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

/// `Resolver` for the path-shape branch (`./foo`, `foo.tgz`,
/// `/abs/path`, `~/dir`, `C:\drive`). Runs **last** in the chain —
/// after named-registry — so a `<alias>:@scope/pkg` specifier reaches
/// the named-registry resolver instead of being misrouted here on
/// the strength of an embedded `/` (`contains_path_sep` in
/// `parse_bare_specifier.rs`). Mirrors
/// [`_resolveFromLocalPath`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/default-resolver/src/index.ts#L146).
///
/// `resolve_latest` returns `Ok(None)` because the equivalent
/// upstream step is folded into `resolveLatestFromLocal` and only
/// fires once for the scheme branch.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalPathResolver {
    pub ctx: LocalResolverContext,
}

impl LocalPathResolver {
    #[must_use]
    pub fn new(ctx: LocalResolverContext) -> Self {
        Self { ctx }
    }
}

impl Resolver for LocalPathResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let Some(wd) = wanted_local(wanted_dependency) else {
                return Ok(None);
            };
            let local_opts = local_options(opts);
            let Some(result) = resolve_from_local_path(&self.ctx, &wd, &local_opts)
                .await
                .map_err(|err| Box::new(err) as ResolveError)?
            else {
                return Ok(None);
            };
            Ok(Some(into_chain_result(result, wanted_dependency)))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async move { Ok(None) })
    }
}

/// Combined scheme-then-path resolver. Kept for tests and one-off
/// chains that don't need the split, but the production chain in
/// `install_without_lockfile.rs` uses [`LocalSchemeResolver`] and
/// [`LocalPathResolver`] separately so the named-registry resolver
/// can slot in between them — matching upstream's chain order.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalResolver {
    pub ctx: LocalResolverContext,
}

impl LocalResolver {
    #[must_use]
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
        let Some(wd) = wanted_local(wanted_dependency) else {
            return Ok(None);
        };
        let local_opts = local_options(opts);

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

fn wanted_local(wanted_dependency: &WantedDependency) -> Option<WantedLocalDependency> {
    let bare = wanted_dependency.bare_specifier.clone()?;
    Some(WantedLocalDependency {
        bare_specifier: bare,
        injected: wanted_dependency.injected.unwrap_or(false),
    })
}

fn local_options(opts: &ResolveOptions) -> LocalResolverOptions {
    LocalResolverOptions {
        project_dir: opts.project_dir.clone(),
        lockfile_dir: Some(opts.lockfile_dir.clone()),
        current_pkg: None,
        update: match opts.update {
            UpdateBehavior::Off => LocalResolverUpdate::Off,
            UpdateBehavior::Compatible | UpdateBehavior::Latest => LocalResolverUpdate::On,
        },
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
    chain.alias.clone_from(&wanted_dependency.alias);
    chain
}
