//! Chain-friendly wrappers that implement
//! [`pnpm_resolving_resolver_base::Resolver`] over the free
//! functions in [`super::local_resolver`].
//!
//! The resolver chain interleaves the local-scheme step ahead of the
//! runtime / named-registry resolvers and runs the local-path step
//! last. That split is expressed as two separate [`Resolver`] impls:
//! [`LocalSchemeResolver`] (claims `link:` / `file:` / `workspace:`)
//! and [`LocalPathResolver`] (claims bare path-shape specifiers like
//! `./foo` or `foo.tgz`). [`LocalResolver`] is the combined form
//! kept for tests and one-off chains that don't need the split.

use crate::{
    directory_manifest::WorkspaceProjectManifests,
    local_resolver::{
        LocalCurrentPkg,
        LocalResolverContext,
        LocalResolverOptions,
        LocalResolverUpdate,
        ResolveLocalError,
        resolve_from_local_path,
        resolve_from_local_scheme,
        resolve_latest_from_local,
    },
    parse_bare_specifier::WantedLocalDependency,
};
use std::sync::{
    Arc,
    Mutex,
    PoisonError,
};

use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_resolver_base::{
    LatestQuery,
    ResolveError,
    ResolveFuture,
    ResolveLatestFuture,
    ResolveOptions,
    ResolveResult,
    Resolver,
    UpdateBehavior,
    WantedDependency,
};

/// `Resolver` for the local-scheme branch (`link:` / `file:` /
/// `workspace:`). Sits between the tarball resolver and the runtime
/// / named-registry resolvers in the chain.
#[derive(Debug, Default)]
pub struct LocalSchemeResolver {
    pub ctx: LocalResolverContext,
    workspace_projects: WorkspaceProjectsMemo,
}

impl LocalSchemeResolver {
    #[must_use]
    pub fn new(ctx: LocalResolverContext) -> Self {
        Self { ctx, workspace_projects: WorkspaceProjectsMemo::default() }
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
            let local_opts = local_options(opts, &self.workspace_projects);
            let Some(result) = resolve_from_local_scheme(&self.ctx, &wd, &local_opts)
                .await
                .map_err(into_resolve_error)?
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
/// `/abs/path`, `~/dir`, `C:\drive`). Runs **last** in the chain
/// (after named-registry) so a `<alias>:@scope/pkg` specifier reaches
/// the named-registry resolver instead of being misrouted here on
/// the strength of an embedded `/` (`contains_path_sep` in
/// `parse_bare_specifier.rs`).
///
/// `resolve_latest` returns `Ok(None)` because the latest-version
/// step is handled by [`resolve_latest_from_local`] and only fires
/// once, for the scheme branch.
#[derive(Debug, Default)]
pub struct LocalPathResolver {
    pub ctx: LocalResolverContext,
    workspace_projects: WorkspaceProjectsMemo,
}

impl LocalPathResolver {
    #[must_use]
    pub fn new(ctx: LocalResolverContext) -> Self {
        Self { ctx, workspace_projects: WorkspaceProjectsMemo::default() }
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
            let local_opts = local_options(opts, &self.workspace_projects);
            let Some(result) = resolve_from_local_path(&self.ctx, &wd, &local_opts)
                .await
                .map_err(into_resolve_error)?
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
/// chains that don't need the split.
#[derive(Debug, Default)]
pub struct LocalResolver {
    pub ctx: LocalResolverContext,
    workspace_projects: WorkspaceProjectsMemo,
}

impl LocalResolver {
    #[must_use]
    pub fn new(ctx: LocalResolverContext) -> Self {
        Self { ctx, workspace_projects: WorkspaceProjectsMemo::default() }
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
        let local_opts = local_options(opts, &self.workspace_projects);

        if let Some(result) = resolve_from_local_scheme(&self.ctx, &wd, &local_opts)
            .await
            .map_err(into_resolve_error)?
        {
            return Ok(Some(into_chain_result(result, wanted_dependency)));
        }

        if let Some(result) = resolve_from_local_path(&self.ctx, &wd, &local_opts)
            .await
            .map_err(into_resolve_error)?
        {
            return Ok(Some(into_chain_result(result, wanted_dependency)));
        }

        Ok(None)
    }
}

/// Boxes the codes the tree walker downcasts outermost.
fn into_resolve_error(err: ResolveLocalError) -> ResolveError {
    match err {
        ResolveLocalError::UnsupportedProtocol(err) => Box::new(err),
        err => Box::new(err),
    }
}

fn wanted_local(wanted_dependency: &WantedDependency) -> Option<WantedLocalDependency> {
    let bare = wanted_dependency.bare_specifier.clone()?;
    Some(WantedLocalDependency {
        bare_specifier: bare,
        injected: wanted_dependency.injected.unwrap_or(false),
    })
}

/// The [`WorkspaceProjectManifests`] of the workspace packages the last
/// resolve carried. An install hands every resolve the same map, so its
/// by-directory index is built once.
#[derive(Debug, Default)]
struct WorkspaceProjectsMemo(Mutex<Option<Arc<WorkspaceProjectManifests>>>);

impl WorkspaceProjectsMemo {
    fn of(&self, opts: &ResolveOptions) -> Option<Arc<WorkspaceProjectManifests>> {
        let packages = opts.project.workspace_packages.as_ref()?;
        let mut memo = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(projects) = memo
            .as_ref()
            .filter(|projects| projects.views(packages))
        {
            return Some(Arc::clone(projects));
        }
        let projects = Arc::new(WorkspaceProjectManifests::new(Arc::clone(packages)));
        *memo = Some(Arc::clone(&projects));
        Some(projects)
    }
}

fn local_options(
    opts: &ResolveOptions,
    workspace_projects: &WorkspaceProjectsMemo,
) -> LocalResolverOptions {
    LocalResolverOptions {
        project_dir: opts.project.project_dir.clone(),
        lockfile_dir: Some(opts.project.lockfile_dir.clone()),
        workspace_dir: opts.project.workspace_dir.clone(),
        current_pkg: opts.refresh.current_pkg
            .as_ref()
            .filter(|current| matches!(current.resolution, LockfileResolution::Tarball(_)))
            .map(|current| LocalCurrentPkg {
                id: current.id.clone(),
                resolution: current.resolution.clone(),
                manifest: current.manifest.as_ref().map(Arc::clone),
            }),
        update: match opts.refresh.update {
            UpdateBehavior::Compatible | UpdateBehavior::Latest => LocalResolverUpdate::On,
            UpdateBehavior::Off | UpdateBehavior::Patches => LocalResolverUpdate::Off,
        },
        inject_workspace_packages: opts.project.inject_workspace_packages,
        workspace_projects: workspace_projects.of(opts),
    }
}

/// Thread the alias from the [`WantedDependency`] onto the chain
/// result so the install layer can address the resolved package by
/// the manifest key.
fn into_chain_result(
    result: crate::local_resolver::LocalResolveResult,
    wanted_dependency: &WantedDependency,
) -> ResolveResult {
    let mut chain: ResolveResult = result.into();
    chain.alias.clone_from(&wanted_dependency.alias);
    chain
}
