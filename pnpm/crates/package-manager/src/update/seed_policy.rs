use super::{
    CatalogCtx, LatestResolverChain, LatestRewriteCtx, MatchedRewriteInputs, UpdateError,
    WorkspaceLinkTarget, emit_latest_ignored, latest_specifier, record_matched_direct_update,
    selectors::{ParsedSelector, expand_update_selectors, insert_update_target},
    workspace_specifier,
};
use crate::{ImporterUpdateSeedPolicy, UpdateSeedPolicy};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_matcher::create_matcher;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_deps_resolver::{UpdateDepth, UpdateTargets};
use pnpm_resolving_resolver_base::{PreferredVersions, WorkspacePackages};
use std::collections::{BTreeMap, HashMap};

pub(super) fn selected_seed_policy(
    patches: bool,
    policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
    depth: usize,
) -> UpdateSeedPolicy {
    if patches {
        return UpdateSeedPolicy::RefreshRevisions;
    }
    UpdateSeedPolicy::ByImporter { policies, max_depth: UpdateDepth::new(depth) }
}
/// What every branch of the seed-policy decision reads.
pub(super) struct UpdateScope<'a> {
    pub(super) selectors: &'a [ParsedSelector],
    /// The direct dependencies as the manifest declared them before the
    /// update rewrote anything: `(name, group, specifier)`.
    pub(super) direct: &'a [(String, DependencyGroup, String)],
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) config: &'a Config,
    pub(super) latest: bool,
    pub(super) save: bool,
    pub(super) depth: usize,
    pub(super) max_depth: UpdateDepth,
    pub(super) range_spec_style: RangeSpecStyle,
    pub(super) updates_all_groups: bool,
    pub(super) use_name_matcher: bool,
}
/// What the branches accumulate on the way to a seed policy.
#[derive(Default)]
pub(super) struct UpdatePlan {
    /// Names whose lockfile pins the resolve must not reuse.
    pub(super) drop_targets: UpdateTargets,
    /// Manifest declarations to rewrite: `(name, group, specifier)`.
    pub(super) rewrites: Vec<(String, DependencyGroup, String)>,
    /// A compatible bump cannot name its version before the resolve, so the
    /// matched names are collected here and the install reports back what it
    /// settled on.
    pub(super) bump_targets: HashMap<String, (DependencyGroup, String)>,
    pub(super) preferred_versions_override: PreferredVersions,
}
impl UpdatePlan {
    fn drop_only(&mut self, max_depth: UpdateDepth) -> UpdateSeedPolicy {
        UpdateSeedPolicy::DropOnly { targets: std::mem::take(&mut self.drop_targets), max_depth }
    }
}
/// The seed policy this update runs under, or `None` when nothing it names is
/// updatable and the command is a no-op.
pub(super) async fn select_seed_policy<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
    workspace: (Option<&WorkspacePackages>, Vec<WorkspaceLinkTarget>),
) -> Result<Option<UpdateSeedPolicy>, UpdateError> {
    let (workspace_packages, workspace_targets) = workspace;
    if let Some(workspace_packages) = workspace_packages.filter(|_| !workspace_targets.is_empty()) {
        return Ok(Some(workspace_seed_policy(scope, plan, workspace_targets, workspace_packages)));
    }
    if scope.selectors.is_empty() {
        return all_direct_seed_policy::<Reporter>(
            scope,
            plan,
            rewrite_ctx,
            latest_chain,
            catalog_ctx,
        )
        .await
        .map(Some);
    }
    if scope.use_name_matcher {
        return Ok(Some(name_matched_seed_policy(scope, plan)));
    }
    selector_seed_policy::<Reporter>(scope, plan, rewrite_ctx, latest_chain, catalog_ctx).await
}
/// `--workspace`: every matched dependency is relinked to the workspace
/// project that provides it.
pub(super) fn workspace_seed_policy(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    workspace_targets: Vec<WorkspaceLinkTarget>,
    workspace_packages: &WorkspacePackages,
) -> UpdateSeedPolicy {
    for target in workspace_targets {
        let specifier = workspace_specifier(
            &target,
            &workspace_packages[&target.name],
            scope.config.save_workspace_protocol,
            scope.range_spec_style,
        );
        plan.drop_targets.insert(target.name.clone(), None);
        plan.rewrites.push((target.name, target.group, specifier));
    }
    plan.drop_only(scope.max_depth)
}
/// No selector: every included direct dependency updates.
pub(super) async fn all_direct_seed_policy<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
) -> Result<UpdateSeedPolicy, UpdateError> {
    // `updateConfig.ignoreDependencies` applies only when no selector was
    // supplied and remains scoped by the included direct groups.
    let ignore_patterns =
        scope.config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
    let ignore_matcher = (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
    let is_ignored =
        |name: &str| ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name));
    if scope.latest && !scope.save {
        emit_latest_ignored::<Reporter>(rewrite_ctx.manifest);
    }
    for (name, group, previous) in scope.direct {
        if is_ignored(name) {
            continue;
        }
        record_direct_update(
            scope,
            plan,
            rewrite_ctx,
            latest_chain,
            catalog_ctx,
            (name, *group, previous),
        )
        .await?;
    }
    if scope.updates_all_groups && ignore_patterns.is_empty() {
        // A bare, ungated update re-resolves the whole graph.
        return Ok(UpdateSeedPolicy::DropAll { max_depth: scope.max_depth });
    }
    let nothing_dropped = plan.drop_targets.is_empty();
    widen_drop_targets_to_lockfile(scope, plan, nothing_dropped, &is_ignored);
    Ok(plan.drop_only(scope.max_depth))
}
/// One direct dependency of a selector-less update.
pub(super) async fn record_direct_update(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
    declared: (&String, DependencyGroup, &String),
) -> Result<(), UpdateError> {
    let (name, group, previous) = declared;
    if scope.latest
        && scope.save
        && let Some(specifier) =
            latest_specifier(rewrite_ctx, latest_chain, catalog_ctx, name, previous).await?
    {
        plan.rewrites.push((name.clone(), group, specifier));
    }
    if scope.save && !scope.latest {
        plan.bump_targets.entry(name.clone()).or_insert_with(|| (group, previous.clone()));
    }
    plan.drop_targets.insert(name.clone(), None);
    Ok(())
}
/// An update that covers every group also drops the pins of the packages only
/// the lockfile names, so nothing transitive stays behind.
pub(super) fn widen_drop_targets_to_lockfile(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    nothing_dropped: bool,
    is_ignored: &impl Fn(&str) -> bool,
) {
    if !scope.updates_all_groups || (scope.latest && nothing_dropped) {
        return;
    }
    let Some(snapshots) = scope.lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) else {
        return;
    };
    for key in snapshots.keys() {
        let name = key.name.to_string();
        if !is_ignored(&name) {
            plan.drop_targets.insert(name, None);
        }
    }
}
/// Bare-name selectors with a depth: every matching name updates, at any
/// depth.
pub(super) fn name_matched_seed_policy(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
) -> UpdateSeedPolicy {
    let patterns =
        scope.selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
    let matcher = create_matcher(&patterns);
    for (name, group, previous) in scope.direct {
        if !matcher.matches(name) {
            continue;
        }
        if scope.save {
            plan.bump_targets.entry(name.clone()).or_insert_with(|| (*group, previous.clone()));
        }
        plan.drop_targets.insert(name.clone(), None);
    }
    widen_drop_targets_by_matcher(scope.lockfile, plan, &matcher);
    plan.drop_only(scope.max_depth)
}
/// Lockfile names keep transitive-only matches in the update scope.
pub(super) fn widen_drop_targets_by_matcher(
    lockfile: Option<&Lockfile>,
    plan: &mut UpdatePlan,
    matcher: &pnpm_matcher::Matcher,
) {
    let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) else {
        return;
    };
    for key in snapshots.keys() {
        let name = key.name.to_string();
        if matcher.matches(&name) {
            plan.drop_targets.insert(name, None);
        }
    }
}
/// Selectors that may name a version: only what they match updates.
pub(super) async fn selector_seed_policy<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
) -> Result<Option<UpdateSeedPolicy>, UpdateError> {
    let patterns =
        scope.selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
    let matcher = create_matcher(&patterns);
    let expanded = expand_update_selectors(scope.selectors);
    let matched_direct = scope
        .direct
        .iter()
        .filter(|(name, _, _)| matcher.matches(name))
        .cloned()
        .collect::<Vec<_>>();
    if matched_direct.is_empty() {
        // An unmatched `--latest` selector is a no-op. Deeper versioned
        // selectors can still target lockfile names but cannot force that
        // version.
        if scope.depth == 0 || scope.latest {
            return Ok(None);
        }
        widen_drop_targets_by_selectors(scope.lockfile, plan, &expanded);
        return Ok(Some(plan.drop_only(scope.max_depth)));
    }
    if scope.latest && !scope.save {
        emit_latest_ignored::<Reporter>(rewrite_ctx.manifest);
    }
    for (name, group, previous) in &matched_direct {
        record_matched_direct_update::<Reporter>(
            scope,
            plan,
            MatchedRewriteInputs { rewrite_ctx, latest_chain, catalog_ctx, expanded: &expanded },
            (name, *group, previous),
        )
        .await?;
    }
    Ok(Some(plan.drop_only(scope.max_depth)))
}
pub(super) fn widen_drop_targets_by_selectors(
    lockfile: Option<&Lockfile>,
    plan: &mut UpdatePlan,
    expanded: &[ParsedSelector],
) {
    let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) else {
        return;
    };
    let target_matcher = create_matcher(
        &expanded.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>(),
    );
    for key in snapshots.keys() {
        let name = key.name.to_string();
        if target_matcher.matches(&name) {
            insert_update_target(&mut plan.drop_targets, expanded, &name);
        }
    }
}
/// One project's own share of a recursive update, as the workspace-wide
/// per-importer policy records it. `None` leaves the importer out of the
/// policy map, which reads as keeping every pin it has.
pub(super) fn importer_seed_policy(
    seed_policy: UpdateSeedPolicy,
) -> Option<ImporterUpdateSeedPolicy> {
    match seed_policy {
        UpdateSeedPolicy::KeepAll => None,
        UpdateSeedPolicy::DropAll { .. } => Some(ImporterUpdateSeedPolicy::DropAll),
        UpdateSeedPolicy::DropOnly { targets, .. } => {
            Some(ImporterUpdateSeedPolicy::DropOnly(targets))
        }
        UpdateSeedPolicy::KeepAllResolveAll
        | UpdateSeedPolicy::FixLockfile
        | UpdateSeedPolicy::RefreshRevisions => {
            unreachable!("manifest preparation never uses a whole-graph seed policy")
        }
        UpdateSeedPolicy::ByImporter { .. } => {
            unreachable!("per-manifest preparation never produces importer policies")
        }
    }
}
