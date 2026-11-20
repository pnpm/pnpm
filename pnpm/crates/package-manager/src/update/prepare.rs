pub(super) mod selection;
use selection::should_reject_unmatched_selection;

use super::{
    UpdateError, UpdateOptions, UpdateResources, UpdateSeed,
    catalogs::{
        CatalogCtx, CatalogUpdates, ensure_catalog_ctx, merge_catalogs,
        read_catalog_ctx_with_catalogs, reconcile_catalog_rewrites,
    },
    latest::{LatestResolverChain, LatestRewriteCtx},
    overrides::{BareOverrideEntry, OverriddenDirect},
    seed_policy::{LockedImporter, UpdatePlan, UpdateScope, select_seed_policy},
    selectors::{
        ParsedSelector, parse_selectors, reject_versioned_selectors,
        reject_versions_of_indirect_update_specs,
    },
    workspace::workspace_targets,
};
use crate::{
    DIRECT_GROUPS, ImporterUpdateSeedPolicy, UpdateSeedPolicy, VersionsOverrider,
    emit_initial_package_manifest,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::PreferredVersions;
use std::{collections::BTreeMap, path::Path};

pub(super) struct UpdatePreparation {
    seed_policy: UpdateSeedPolicy,
    preferred_versions_override: PreferredVersions,
    pub(super) persist_manifest: bool,
    /// Direct dependencies whose declared range the install may move onto
    /// the version it resolves, each mapped to the group and specifier the
    /// manifest declares for it. See [`crate::ManifestSpecBumps`].
    pub(super) bump_targets: Vec<(String, DependencyGroup, String)>,
    pub(super) catalogs: CatalogUpdates,
    /// Override entries the update moves (`(bare-name key, new value)`),
    /// for the run to write back into `pnpm-workspace.yaml` and resolve
    /// against in the meantime.
    pub(super) updated_overrides: Vec<(String, String)>,
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
            catalogs_override: self.catalogs.catalogs_override.take(),
        }
    }
}
#[derive(Default)]
pub(super) struct SelectedUpdatePreparation {
    pub(super) seed_policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
    preferred_versions_override: PreferredVersions,
    pub(super) persist_indices: Vec<usize>,
    /// [`UpdatePreparation::bump_targets`] per importer id.
    pub(super) bump_targets: BTreeMap<String, Vec<(String, DependencyGroup, String)>>,
    pub(super) catalogs: CatalogUpdates,
    work: SelectedUpdateWork,
    /// Override entries to move, merged from every prepared project.
    pub(super) updated_overrides: Vec<(String, String)>,
}
#[derive(Default, PartialEq, Eq)]
enum SelectedUpdateWork {
    #[default]
    None,
    IgnoredDirectMatch,
    Prepared,
}
pub(super) async fn prepare_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    importer_id: &str,
    update: UpdateOptions<'_>,
    owned: &UpdateResources,
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
) -> Result<Option<UpdatePreparation>, UpdateError> {
    let Some(decision) = decide_update::<Reporter>(
        manifest,
        importer_id,
        update,
        owned,
        catalogs_seed,
        latest_chain,
    )
    .await?
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
    importer_id: &str,
    update: UpdateOptions<'_>,
    owned: &UpdateResources,
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
) -> Result<Option<UpdateDecision>, UpdateError> {
    let selectors = parse_selectors(update.selection.packages);
    reject_versioned_selectors(&update.version, update.selection.packages)?;
    // Snapshot direct dependencies before mutation so matching and rewrites
    // both see the original manifest shape.
    let direct = declared_direct(manifest, &owned.include_direct);
    // Catalogs stay lazy unless an earlier selected project already produced
    // the complete in-memory catalog set for this batch.
    let mut catalog_ctx = catalogs_seed
        .map(|catalogs| read_catalog_ctx_with_catalogs(manifest, update.config, catalogs.clone()))
        .transpose()?;
    let overridden_direct = overridden_direct(manifest, update, &direct, &mut catalog_ctx)?;
    let scope = update_scope(update, owned, importer_id, &selectors, &direct, &overridden_direct);
    let mut plan = UpdatePlan::default();
    let Some(seed_policy) = select_seed_policy::<Reporter>(
        &scope,
        &mut plan,
        &LatestRewriteCtx {
            manifest,
            config: update.config,
            http_client_arc: &owned.http_client_arc,
            resolution_observer: owned.resolution_observer.as_ref(),
            range_spec_style: scope.range_spec_style(),
            lockfile_only: update.lockfile_only,
        },
        latest_chain,
        &mut catalog_ctx,
        (update.selection.workspace_packages, workspace_targets(update, &selectors, &direct)?),
    )
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(UpdateDecision { plan, seed_policy, direct, catalog_ctx }))
}
pub(super) fn update_scope<'a>(
    update: UpdateOptions<'a>,
    owned: &UpdateResources,
    importer_id: &'a str,
    selectors: &'a [ParsedSelector],
    direct: &'a [(String, DependencyGroup, String)],
    overridden_direct: &'a [OverriddenDirect],
) -> UpdateScope<'a> {
    UpdateScope {
        selectors,
        direct,
        overridden_direct,
        lockfile: LockedImporter { document: update.lockfile.document, id: importer_id },
        config: update.config,
        version: update.version,
        selection: update.selection,
        updates_all_groups: updates_all_groups(&owned.include_direct),
    }
}

fn overridden_direct(
    manifest: &PackageManifest,
    update: UpdateOptions<'_>,
    direct: &[(String, DependencyGroup, String)],
    catalog_ctx: &mut Option<CatalogCtx>,
) -> Result<Vec<OverriddenDirect>, UpdateError> {
    if update.config.overrides.as_ref().is_none_or(indexmap::IndexMap::is_empty) {
        return Ok(Vec::new());
    }
    let catalogs = &ensure_catalog_ctx(catalog_ctx, manifest, update.config)?.catalogs;
    let parsed = crate::install::parse_config_overrides(update.config, catalogs)
        .map_err(|error| UpdateError::Install(error.into()))?
        .unwrap_or_default();
    let project_dir = super::manifest_dir(manifest);
    let lockfile_root = crate::install::lockfile_root_dir(update.config, project_dir)
        .map_err(UpdateError::FindWorkspaceDir)?;
    let overrider =
        VersionsOverrider::new(&parsed, update.config.root_project_manifest_dir(&lockfile_root))
            .with_catalogs(catalogs);
    Ok(collect_overridden_direct(manifest, direct, &overrider, update.config))
}

fn collect_overridden_direct(
    manifest: &PackageManifest,
    direct: &[(String, DependencyGroup, String)],
    overrider: &VersionsOverrider,
    config: &Config,
) -> Vec<OverriddenDirect> {
    let matcher = overrider.dependency_matcher(manifest.value());
    let project_overrides = project_overrides(manifest, config);
    let matched = direct
        .iter()
        .filter(|(name, group, previous)| matcher.matches(*group, name, previous))
        .map(|(name, group, previous)| {
            let bare_override = matcher
                .governing_override(*group, name, previous)
                .and_then(|governing| {
                    bare_override_entry(name, governing, config, project_overrides)
                });
            (name.clone(), *group, bare_override)
        })
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return Vec::new();
    }
    let mut effective = manifest.clone();
    overrider.apply(&mut effective, Some(super::manifest_dir(manifest)));
    matched
        .into_iter()
        .map(|(name, group, bare_override)| OverriddenDirect {
            effective_specifier: effective
                .dependencies([group])
                .find_map(|(alias, specifier)| (alias == name).then(|| specifier.to_string())),
            bare_override,
            name,
            group,
        })
        .collect()
}

/// The override entry a targeted update may move for `name`, given the entry
/// that governs it: one keyed by the package's own name, with no parent scope,
/// no range scope on the target, and no convergence form. A parent-scoped
/// entry does not govern this project's own edge, and a range-scoped one would
/// stop matching once the declaration moved. A dependency-type key such as
/// `foo:prod` qualifies: it matches every declaration of its group whatever
/// the range, so it holds the declaration the way a bare key does, though the
/// update can only report it.
///
/// The value is the one written in `pnpm-workspace.yaml`. Config resolves a
/// `$` reference to the specifier it points at, which the update must not
/// write back over the reference.
fn bare_override_entry(
    name: &str,
    governing: &pnpm_config_parse_overrides::VersionOverride,
    config: &Config,
    project_overrides: Option<&indexmap::IndexMap<String, String>>,
) -> Option<BareOverrideEntry> {
    if governing.parent_pkg.is_some()
        || governing.target_pkg.bare_specifier.is_some()
        || governing.converge
        || governing.target_pkg.name != name
    {
        return None;
    }
    if let Some(project_overrides) = project_overrides {
        return project_overrides
            .get(&governing.selector)
            .map(|value| BareOverrideEntry {
                key: governing.selector.clone(),
                value: value.clone(),
                resolved: governing.new_bare_specifier.clone(),
                writable: false,
            });
    }
    let written = config.explicit_settings
        .get("overrides")
        .and_then(|overrides| overrides.get(&governing.selector))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    written
        .or_else(|| {
            config.overrides
                .as_ref()?
                .get(&governing.selector)
                .cloned()
        })
        .map(|value| BareOverrideEntry {
            key: governing.selector.clone(),
            value,
            resolved: governing.new_bare_specifier.clone(),
            writable: governing.target_pkg.dep_type.is_none(),
        })
}
/// The `overrides` a project's `packageConfigs` entry sets for it, when the
/// workspace applies `packageConfigs` at all. They replace the workspace-wide
/// map for that project, and the workspace
/// manifest writer edits only the workspace-wide map, so an entry from them
/// is reported rather than moved.
fn project_overrides<'config>(
    manifest: &PackageManifest,
    config: &'config Config,
) -> Option<&'config indexmap::IndexMap<String, String>> {
    if !config.applies_package_configs() {
        return None;
    }
    let name = manifest.value().get("name")?.as_str()?;
    config.package_configs
        .as_ref()?
        .get(name)?
        .overrides
        .as_ref()
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
    DIRECT_GROUPS
        .iter()
        .all(|group| include_direct.contains(group))
}
pub(super) fn apply_update_decision<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    update: UpdateOptions<'_>,
    decision: UpdateDecision,
) -> Result<UpdatePreparation, UpdateError> {
    let UpdateDecision {
        mut plan,
        seed_policy,
        direct,
        mut catalog_ctx,
    } = decision;
    // Reconcile only manifest rewrites. Existing `catalog:` references retain
    // their group, and non-manual catalog modes may promote direct versions.
    let mut updated_catalogs = Catalogs::new();
    let workspace_dir_for_catalogs = reconcile_catalog_rewrites::<Reporter>(
        manifest,
        update.config,
        update.version.reaches_past_declared_range(),
        &direct,
        &mut plan.rewrites,
        &mut catalog_ctx,
        &mut updated_catalogs,
    )?;
    // `--no-save` still mutates the in-memory manifest used for resolution,
    // while leaving package.json and reporter manifest events untouched.
    let persist_manifest = update.version.save && !plan.rewrites.is_empty();
    if persist_manifest {
        emit_initial_package_manifest::<Reporter>(manifest);
    }
    apply_rewrites(manifest, &plan.rewrites)?;
    Ok(UpdatePreparation {
        seed_policy,
        preferred_versions_override: plan.preferred_versions_override,
        persist_manifest,
        bump_targets: plan.bump_targets,
        catalogs: CatalogUpdates {
            catalogs_override: merged_catalogs_override(catalog_ctx.as_ref(), &updated_catalogs),
            updated_catalogs,
            workspace_dir_for_catalogs,
        },
        updated_overrides: plan.updated_overrides,
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
    update: UpdateOptions<'_>,
    owned: &UpdateResources,
) -> Result<SelectedUpdatePreparation, UpdateError> {
    // One picker across every selected project: it is created on first
    // use, so a selection that resolves no `latest` tag never builds one.
    let mut latest_chain = None;
    let mut prepared_all = SelectedUpdatePreparation::default();

    // Once per command, across every selected project: a selector that is a
    // direct dependency of one project is legitimately versioned even where a
    // sibling only reaches it transitively. `--depth 0` reports
    // `NoPackageInDependencies` instead, and `--latest` / `--tag` reject
    // versioned selectors outright.
    if !update.version.reaches_past_declared_range() && update.selection.depth > 0 {
        let selectors = parse_selectors(update.selection.packages);
        let manifests = selected_indices
            .iter()
            .map(|&index| &projects[index].manifest)
            .collect::<Vec<_>>();
        reject_versions_of_indirect_update_specs::<Reporter>(
            &selectors,
            &manifests,
            &owned.include_direct,
            &workspace_root.to_string_lossy(),
        )?;
    }

    for &index in selected_indices {
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(workspace_root, &projects[index].root_dir);
        let Some(prepared) = prepare_manifest::<Reporter>(
            &mut projects[index].manifest,
            &importer_id,
            update,
            owned,
            prepared_all.catalogs.catalogs_override.as_ref(),
            &mut latest_chain,
        )
        .await?
        else {
            prepared_all.record_ignored_match(&projects[index].manifest, update, owned);
            continue;
        };
        prepared_all.merge(index, importer_id, prepared);
    }

    // A recursive `--latest` that matches nothing is an error, unlike the
    // single-project one that quietly returns: with no project left to
    // mutate there is nothing for the run to have meant.
    if should_reject_unmatched_selection(update, &prepared_all) {
        return Err(UpdateError::NoPackageInDependencies);
    }

    Ok(prepared_all)
}
