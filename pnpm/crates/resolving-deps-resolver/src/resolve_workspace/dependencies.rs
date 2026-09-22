use super::{
    DependencyGroup,
    InitializedImporters,
    PassSettings,
    ResolveImporterError,
    ResolveImporterOptions,
    ResolveWorkspaceResult,
    Resolver,
    WorkspaceImporter,
    WorkspaceResolveOptions,
    WorkspaceTreeCtx,
    finish,
    init_importers,
    run_hoist_rounds,
    share_root_deps,
    sorted_importers,
    time_cutoff,
};
use pnpm_resolving_resolver_base::PreferredVersions;
use std::{
    collections::BTreeMap,
    sync::Arc,
};

/// A dependency tree before automatic peer installation and peer-context resolution.
/// Dropping an intermediate tree avoids all peer processing for that tree.
pub struct ResolvedWorkspaceDependencies {
    workspace: Arc<WorkspaceTreeCtx>,
    settings: PassSettings,
    initialized: InitializedImporters,
    time: BTreeMap<String, String>,
}

impl ResolvedWorkspaceDependencies {
    /// Concrete versions for reachable package names with multiple versions.
    /// Reads the tree's cached version index without creating a peer-resolved graph.
    #[must_use]
    pub fn duplicate_versions(&self) -> PreferredVersions {
        self.workspace.duplicate_versions()
    }

    /// Install missing peers, then resolve peer contexts once for the settled tree.
    pub async fn resolve_peers<Chain>(
        mut self,
        resolver: &Chain,
    ) -> Result<ResolveWorkspaceResult, ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        share_root_deps(&mut self.initialized.states)?;
        run_hoist_rounds(resolver, &mut self.initialized.states, &self.workspace).await?;
        Ok(finish(&self.settings, self.workspace, self.initialized, self.time))
    }
}

/// Resolve regular dependencies without installing missing peers or constructing peer contexts.
/// Call [`ResolvedWorkspaceDependencies::resolve_peers`] after version convergence has settled.
pub async fn resolve_workspace_dependencies<'a, Chain, BuildImporterOptions>(
    resolver: &Chain,
    importers: &[WorkspaceImporter<'a>],
    dependency_groups: &[DependencyGroup],
    opts: WorkspaceResolveOptions,
    per_importer_options: BuildImporterOptions,
) -> Result<ResolvedWorkspaceDependencies, ResolveImporterError>
where
    Chain: Resolver + ?Sized,
    BuildImporterOptions: FnMut(&WorkspaceImporter<'a>) -> ResolveImporterOptions,
{
    let (workspace, settings) = opts.split();
    let sorted = sorted_importers(importers, per_importer_options, &settings);
    let cutoff = time_cutoff(resolver, &sorted, dependency_groups, &settings).await;
    let initialized =
        init_importers(resolver, sorted, dependency_groups, &cutoff, &settings, &workspace).await?;
    Ok(ResolvedWorkspaceDependencies { workspace, settings, initialized, time: cutoff.time })
}
