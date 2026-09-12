use super::{
    UpdateError, UpdateOwned, UpdateSeed, UpdateView,
    catalogs::{
        CatalogCtx, merge_catalogs, read_catalog_ctx_with_catalogs, reconcile_catalog_rewrites,
    },
    latest::{LatestResolverChain, LatestRewriteCtx},
    seed_policy::{
        UpdatePlan, UpdateScope, importer_seed_policy, select_seed_policy, selected_seed_policy,
    },
    selectors::{
        ParsedSelector, parse_selectors, reject_versioned_latest_selectors,
        reject_versions_of_indirect_update_specs,
    },
    workspace::workspace_targets,
};
use crate::{
    DIRECT_GROUPS, ImporterUpdateSeedPolicy, InstallError, UpdateSeedPolicy,
    emit_initial_package_manifest,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_deps_resolver::UpdateDepth;
use pnpm_resolving_resolver_base::PreferredVersions;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct UpdatePreparation {
    seed_policy: UpdateSeedPolicy,
    preferred_versions_override: PreferredVersions,
    pub(super) persist_manifest: bool,
    /// Direct dependencies whose declared range the install may move onto
    /// the version it resolves, each mapped to the group and specifier the
    /// manifest declares for it. See [`crate::ManifestSpecBumps`].
    pub(super) bump_targets: HashMap<String, (DependencyGroup, String)>,
    pub(super) updated_catalogs: Catalogs,
    catalogs_override: Option<Catalogs>,
    pub(super) workspace_dir_for_catalogs: Option<PathBuf>,
}
impl UpdatePreparation {
    pub(super) fn take_seed(&mut self, patches: bool) -> UpdateSeed {
        UpdateSeed {
            policy: if patches {
                UpdateSeedPolicy::RefreshRevisions
            } else {
                std::mem::replace(&mut self.seed_policy, UpdateSeedPolicy::KeepAll)
            },
            preferred_versions_override: std::mem::take(&mut self.preferred_versions_override),
            catalogs_override: self.catalogs_override.take(),
        }
    }
}
#[derive(Default)]
pub(super) struct SelectedUpdatePreparation {
    pub(super) seed_policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
    preferred_versions_override: PreferredVersions,
    pub(super) persist_indices: Vec<usize>,
    /// [`UpdatePreparation::bump_targets`] per importer id.
    pub(super) bump_targets: BTreeMap<String, HashMap<String, (DependencyGroup, String)>>,
    pub(super) updated_catalogs: Catalogs,
    pub(super) catalogs_override: Option<Catalogs>,
    pub(super) workspace_dir_for_catalogs: Option<PathBuf>,
    pub(super) any_work: bool,
}
impl SelectedUpdatePreparation {
    pub(super) fn take_seed(&mut self, update: UpdateView<'_>) -> UpdateSeed {
        UpdateSeed {
            policy: selected_seed_policy(
                update.patches,
                std::mem::take(&mut self.seed_policies),
                update.depth,
            ),
            preferred_versions_override: std::mem::take(&mut self.preferred_versions_override),
            catalogs_override: self.catalogs_override.take(),
        }
    }

    /// Fold one project's preparation in, under the importer id it was
    /// prepared for.
    fn merge(&mut self, index: usize, importer_id: String, prepared: UpdatePreparation) {
        self.any_work = true;
        for (name, selectors) in prepared.preferred_versions_override {
            self.preferred_versions_override.entry(name).or_default().extend(selectors);
        }
        if !prepared.bump_targets.is_empty() {
            self.bump_targets.insert(importer_id.clone(), prepared.bump_targets);
        }
        if let Some(policy) = importer_seed_policy(prepared.seed_policy) {
            self.seed_policies.insert(importer_id, policy);
        }
        if prepared.persist_manifest {
            self.persist_indices.push(index);
        }
        merge_catalogs(&mut self.updated_catalogs, &prepared.updated_catalogs);
        if let Some(complete_catalogs) = prepared.catalogs_override {
            self.catalogs_override = Some(complete_catalogs);
        }
        if self.workspace_dir_for_catalogs.is_none() {
            self.workspace_dir_for_catalogs = prepared.workspace_dir_for_catalogs;
        }
    }
}
/// A loaded `readPackage` hook paired with the log sink its `context.log`
/// calls are forwarded to.
pub(super) type ReadPackageHook = (Arc<dyn pnpm_hooks::PnpmfileHooks>, pnpm_hooks::LogFn);
pub(super) fn update_read_package_hook<Reporter: self::Reporter>(
    workspace_root: &Path,
    config: &Config,
) -> Result<Option<ReadPackageHook>, UpdateError> {
    let Some(hook) =
        pnpm_hooks::finder::load_pnpmfiles(workspace_root, crate::pnpmfile_selection(config))
            .map_err(UpdateError::MissingPnpmfile)?
    else {
        return Ok(None);
    };
    let log = hook.source_path().map_or_else(
        || Arc::new(|_| {}) as pnpm_hooks::LogFn,
        |from| {
            crate::install_with_fresh_lockfile::hook_log_fn::<Reporter>(
                workspace_root,
                from,
                "readPackage",
            )
        },
    );
    Ok(Some((hook, log)))
}
pub(super) async fn apply_read_package_hook_to_update_manifest(
    manifest: &mut PackageManifest,
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    log: &pnpm_hooks::LogFn,
) -> Result<(), UpdateError> {
    let ctx = pnpm_hooks::HookContext { log: Arc::clone(log), dir: None };
    let value = hook
        .read_package(manifest.value().clone(), ctx)
        .await
        .map_err(InstallError::ReadPackageHook)
        .map_err(UpdateError::Install)?;
    *manifest.value_mut() = (*value).clone();
    Ok(())
}
pub(super) async fn prepare_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    update: UpdateView<'_>,
    owned: &UpdateOwned,
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
) -> Result<Option<UpdatePreparation>, UpdateError> {
    let Some(decision) =
        decide_update::<Reporter>(manifest, update, owned, catalogs_seed, latest_chain).await?
    else {
        return Ok(None);
    };
    apply_update_decision::<Reporter>(manifest, update, decision).map(Some)
}
/// What the seed-policy decision settled: the plan, the policy, the direct
/// dependencies as the manifest declared them, and the catalogs consulted.
pub(super) struct UpdateDecision {
    plan: UpdatePlan,
    seed_policy: UpdateSeedPolicy,
    direct: Vec<(String, DependencyGroup, String)>,
    catalog_ctx: Option<CatalogCtx>,
}
pub(super) async fn decide_update<Reporter: self::Reporter>(
    manifest: &PackageManifest,
    update: UpdateView<'_>,
    owned: &UpdateOwned,
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
) -> Result<Option<UpdateDecision>, UpdateError> {
    let selectors = parse_selectors(update.packages);
    if update.latest {
        reject_versioned_latest_selectors(update.packages, &selectors)?;
    }
    // Snapshot direct dependencies before mutation so matching and rewrites
    // both see the original manifest shape.
    let direct = declared_direct(manifest, &owned.include_direct);
    // Catalogs stay lazy unless an earlier selected project already produced
    // the complete in-memory catalog set for this batch.
    let mut catalog_ctx = catalogs_seed
        .map(|catalogs| read_catalog_ctx_with_catalogs(manifest, catalogs.clone()))
        .transpose()?;
    let scope = update_scope(update, owned, &selectors, &direct);
    let mut plan = UpdatePlan::default();
    let Some(seed_policy) = select_seed_policy::<Reporter>(
        &scope,
        &mut plan,
        &LatestRewriteCtx {
            manifest,
            config: update.config,
            http_client_arc: &owned.http_client_arc,
            resolution_observer: owned.resolution_observer.as_ref(),
            range_spec_style: scope.range_spec_style,
            lockfile_only: update.lockfile_only,
        },
        latest_chain,
        &mut catalog_ctx,
        (update.workspace_packages, workspace_targets(update, &selectors, &direct)?),
    )
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(UpdateDecision { plan, seed_policy, direct, catalog_ctx }))
}
pub(super) fn update_scope<'a>(
    update: UpdateView<'a>,
    owned: &UpdateOwned,
    selectors: &'a [ParsedSelector],
    direct: &'a [(String, DependencyGroup, String)],
) -> UpdateScope<'a> {
    UpdateScope {
        selectors,
        direct,
        lockfile: update.lockfile,
        config: update.config,
        latest: update.latest,
        save: update.save,
        depth: update.depth,
        max_depth: UpdateDepth::new(update.depth),
        // `pacquet update` has no `--save-prefix` flag yet, so `save_exact`
        // selects between an exact pin and the default caret range.
        range_spec_style: RangeSpecStyle::from_save_options(update.save_exact, None),
        updates_all_groups: updates_all_groups(&owned.include_direct),
        // Bare-name selectors with depth update matching names at any depth.
        use_name_matcher: !selectors.is_empty()
            && selectors.iter().all(|selector| selector.version.is_none())
            && update.depth > 0
            && !update.latest,
    }
}
/// The direct dependencies of the groups the update covers, as
/// `(name, group, specifier)`.
pub(super) fn declared_direct(
    manifest: &PackageManifest,
    include_direct: &[DependencyGroup],
) -> Vec<(String, DependencyGroup, String)> {
    include_direct
        .iter()
        .flat_map(|&group| {
            manifest
                .dependencies([group])
                .map(move |(name, spec)| (name.to_string(), group, spec.to_string()))
        })
        .collect()
}
pub(super) fn updates_all_groups(include_direct: &[DependencyGroup]) -> bool {
    DIRECT_GROUPS.iter().all(|group| include_direct.contains(group))
}
pub(super) fn apply_update_decision<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    update: UpdateView<'_>,
    decision: UpdateDecision,
) -> Result<UpdatePreparation, UpdateError> {
    let UpdateDecision { mut plan, seed_policy, direct, mut catalog_ctx } = decision;
    // Reconcile only manifest rewrites. Existing `catalog:` references retain
    // their group, and non-manual catalog modes may promote direct versions.
    let mut updated_catalogs = Catalogs::new();
    let workspace_dir_for_catalogs = reconcile_catalog_rewrites::<Reporter>(
        manifest,
        update.config,
        update.latest,
        &direct,
        &mut plan.rewrites,
        &mut catalog_ctx,
        &mut updated_catalogs,
    )?;
    // `--no-save` still mutates the in-memory manifest used for resolution,
    // while leaving package.json and reporter manifest events untouched.
    let persist_manifest = update.save && !plan.rewrites.is_empty();
    if persist_manifest {
        emit_initial_package_manifest::<Reporter>(manifest);
    }
    apply_rewrites(manifest, &plan.rewrites)?;
    Ok(UpdatePreparation {
        seed_policy,
        preferred_versions_override: plan.preferred_versions_override,
        persist_manifest,
        bump_targets: plan.bump_targets,
        catalogs_override: merged_catalogs_override(catalog_ctx.as_ref(), &updated_catalogs),
        updated_catalogs,
        workspace_dir_for_catalogs,
    })
}
pub(super) fn apply_rewrites(
    manifest: &mut PackageManifest,
    rewrites: &[(String, DependencyGroup, String)],
) -> Result<(), UpdateError> {
    for (name, group, specifier) in rewrites {
        manifest.add_dependency(name, specifier, *group).map_err(UpdateError::UpdateManifest)?;
    }
    Ok(())
}
/// The install must resolve against the complete catalog set even when
/// `--no-save` deliberately skips the workspace-manifest write.
pub(super) fn merged_catalogs_override(
    catalog_ctx: Option<&CatalogCtx>,
    updated_catalogs: &Catalogs,
) -> Option<Catalogs> {
    (!updated_catalogs.is_empty()).then(|| {
        let mut merged = catalog_ctx.map(|ctx| ctx.catalogs.clone()).unwrap_or_default();
        merge_catalogs(&mut merged, updated_catalogs);
        merged
    })
}
pub(super) async fn prepare_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
    workspace_root: &Path,
    update: UpdateView<'_>,
    owned: &UpdateOwned,
) -> Result<SelectedUpdatePreparation, UpdateError> {
    // One picker across every selected project: it is created on first
    // use, so a selection that resolves no `latest` tag never builds one.
    let mut latest_chain = None;
    let mut prepared_all = SelectedUpdatePreparation::default();

    // Once per command, across every selected project: a selector that is a
    // direct dependency of one project is legitimately versioned even where a
    // sibling only reaches it transitively. `--depth 0` reports
    // `NoPackageInDependencies` instead, and `--latest` rejects versioned
    // selectors outright.
    if !update.latest && update.depth > 0 {
        let selectors = parse_selectors(update.packages);
        let manifests =
            selected_indices.iter().map(|&index| &projects[index].manifest).collect::<Vec<_>>();
        reject_versions_of_indirect_update_specs::<Reporter>(
            &selectors,
            &manifests,
            &owned.include_direct,
            &workspace_root.to_string_lossy(),
        )?;
    }

    for &index in selected_indices {
        let Some(prepared) = prepare_manifest::<Reporter>(
            &mut projects[index].manifest,
            update,
            owned,
            prepared_all.catalogs_override.as_ref(),
            &mut latest_chain,
        )
        .await?
        else {
            continue;
        };
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(workspace_root, &projects[index].root_dir);
        prepared_all.merge(index, importer_id, prepared);
    }

    // A recursive `--latest` that matches nothing is an error, unlike the
    // single-project one that quietly returns: with no project left to
    // mutate there is nothing for the run to have meant.
    if update.depth == 0 && !update.packages.is_empty() && !prepared_all.any_work {
        return Err(UpdateError::NoPackageInDependencies);
    }

    Ok(prepared_all)
}
