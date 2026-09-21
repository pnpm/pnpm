#[cfg(test)]
mod tests;

use super::super::{FreshInputs, OwnedInputs, setup::ResolverSetup};
use crate::PolicyExcludes;
use pnpm_config::{Config, NodeLinker};
use pnpm_reporter::Reporter;
use std::sync::Arc;

pub(in super::super) fn start_early_materialization<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    owned: &OwnedInputs,
    setup: &ResolverSetup,
) -> Option<Arc<crate::early_materializer::EarlyMaterializer<Reporter>>> {
    early_materialization_eligible(EarlyMaterializationFit {
        config: install.drivers.config,
        node_linker: install.execution.node_linker,
        lockfile_only: install.execution.lockfile_only,
        materializes_subset: setup.shape.materializes_subset,
        is_hoisted: setup.shape.is_hoisted,
        has_custom_fetcher: setup.chain.custom_fetcher_session.is_some(),
    })
    .then(|| {
        Arc::new(crate::early_materializer::EarlyMaterializer::<Reporter>::new(
            install.drivers.config,
            Arc::clone(&owned.fetching.tarball_mem_cache),
        ))
    })
}
/// What decides whether virtual-store slots may be populated ahead of the
/// lockfile.
#[derive(Clone, Copy)]
pub(in super::super) struct EarlyMaterializationFit<'a> {
    config: &'a Config,
    node_linker: NodeLinker,
    lockfile_only: bool,
    materializes_subset: bool,
    is_hoisted: bool,
    has_custom_fetcher: bool,
}
/// Slots can only be populated ahead of the lockfile where their names do not
/// depend on the whole graph (no global virtual store), where the tarballs are
/// prefetched into the cache the materializer waits on, and where the link
/// phase imports straight from the CAS: the macOS directory-clone cache serves
/// project slots from canonical slots it populates itself.
///
/// `--force` rules it out as wasted work rather than as unsafe: the link
/// phase re-imports every slot under it, so anything populated here
/// would be staged and swapped away again.
pub(in super::super) fn early_materialization_eligible(fit: EarlyMaterializationFit<'_>) -> bool {
    !fit.lockfile_only
        && !fit.materializes_subset
        && !fit.is_hoisted
        && !fit.config.force
        && !fit.config.enable_global_virtual_store
        && !pnpm_deps_restorer::DirCloneCache::eligible(fit.config, fit.node_linker)
        && !fit.has_custom_fetcher
}
/// What rules the override fast path out: anything that can rewrite a
/// manifest, or a resolution the rewrite cannot reproduce.
#[derive(Clone, Copy)]
pub(in super::super) struct FastOverrideFit {
    pub(super) has_pnpmfile_hook: bool,
    pub(super) has_custom_resolvers: bool,
    pub(super) has_patches: bool,
    pub(super) can_fast_update_overrides: bool,
}
pub(in super::super) fn fast_override_eligible(fit: FastOverrideFit) -> bool {
    !fit.has_pnpmfile_hook
        && !fit.has_custom_resolvers
        && !fit.has_patches
        && fit.can_fast_update_overrides
}
/// A dry run never prompts and never persists what a prompt would have
/// settled.
pub(in super::super) fn interactive_policy(
    can_prompt: bool,
    policy_excludes: PolicyExcludes,
    dry_run: bool,
) -> (bool, PolicyExcludes) {
    let policy_excludes = if dry_run { policy_excludes.without_writes() } else { policy_excludes };
    (can_prompt && !dry_run, policy_excludes)
}
