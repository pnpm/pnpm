use super::{
    AddError, AddOwned, AddResolution, AddResolveInputs, AddView, resolve_added_dependency,
    workspace_packages_for_add,
};
use crate::{
    CatalogDecision, DIRECT_GROUPS, InstallError,
    catalog_cleanup::{
        post_install_prune, write_workspace_catalogs, write_workspace_catalogs_selected,
    },
    emit_initial_package_manifest, package_manifest_prefix,
};
use futures_util::{StreamExt, stream::FuturesOrdered};
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, SaveWorkspaceProtocol};
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pnpm_resolving_resolver_base::PreferredVersions;
use std::{collections::HashSet, path::PathBuf, sync::Arc};

pub(super) async fn prepare_selected_add<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    indices: &[usize],
    add: AddView<'_>,
    owned: &AddOwned,
) -> Result<SelectedAddPreparation, AddError> {
    let prepared = prepare_selected_manifests::<Reporter>(projects, indices, add, owned).await?;
    write_workspace_catalogs_selected(
        add.config,
        &prepared.workspace_dir,
        &prepared.updated_catalogs,
        projects,
    )
    .map_err(AddError::WriteWorkspaceManifest)?;
    Ok(prepared)
}
pub(super) fn finish_selected_add<Reporter: self::Reporter>(
    add: AddView<'_>,
    manifest: &PackageManifest,
    projects: &mut [pnpm_workspace::Project],
    indices: &[usize],
    workspace_dir: &std::path::Path,
    ignored_builds: Option<InstallError>,
) -> Result<(), AddError> {
    persist_selected_manifests::<Reporter>(projects, indices)?;

    post_install_prune(add.config, Some(workspace_dir), manifest)
        .map_err(AddError::WriteWorkspaceManifest)?;
    if let Some(ignored_builds) = ignored_builds {
        return Err(AddError::Install(ignored_builds));
    }
    Ok(())
}
pub(super) async fn prepare_single_add<Reporter: self::Reporter>(
    add: AddView<'_>,
    owned: &AddOwned,
    manifest: &mut PackageManifest,
) -> Result<(AddCatalogCtx, Catalogs), AddError> {
    let resolution = AddResolution::new();
    let catalog_ctx = read_catalog_ctx(manifest, add.config)?;
    let workspace_packages = (add.config.link_workspace_packages.enabled_at_depth(0)
        || add.config.save_workspace_protocol != SaveWorkspaceProtocol::Rolling)
        .then(|| workspace_packages_for_add(add.config))
        .flatten();
    let git_source_cache = Arc::new(pnpm_git_fetcher::GitSourceCache::default());
    let updated_catalogs = prepare_manifest::<Reporter>(
        manifest,
        &AddResolveInputs {
            add,
            http_client_arc: &owned.http_client_arc,
            git_source_cache: &git_source_cache,
            resolution: &resolution,
            save_catalog_name: owned.save_catalog_name.as_deref(),
            catalogs: &catalog_ctx.catalogs,
            prefix: &catalog_ctx.prefix,
            workspace_packages: workspace_packages.as_ref(),
        },
        owned.dependency_groups.as_deref(),
    )
    .await?;
    // Write the new catalog entry to `pnpm-workspace.yaml` before the
    // install so the resolver reads it back and the lockfile's
    // `catalogs:` snapshot records the resolved version. The same
    // write runs the `catalogPrune` pass when configured.
    write_workspace_catalogs(
        add.config,
        Some(&catalog_ctx.workspace_dir),
        &updated_catalogs,
        manifest,
    )
    .map_err(AddError::WriteWorkspaceManifest)?;
    Ok((catalog_ctx, updated_catalogs))
}
pub(super) fn catalog_version_requests(
    package_selectors: &[String],
    manifest: &PackageManifest,
    catalogs: &Catalogs,
    lockfile: Option<&Lockfile>,
    config: &Config,
    save_catalog_name: Option<&str>,
) -> (HashSet<String>, PreferredVersions) {
    let mut names = HashSet::new();
    let mut preferred = PreferredVersions::new();
    if config.catalog_mode == pnpm_config::CatalogMode::Manual && save_catalog_name.is_none() {
        return (names, preferred);
    }
    for selector in package_selectors {
        let Some((alias, wanted)) =
            catalog_version_request(selector, manifest, catalogs, lockfile, save_catalog_name)
        else {
            continue;
        };
        crate::install_with_fresh_lockfile::prefer_requested_version(
            &mut preferred,
            &alias,
            &wanted,
        );
        names.insert(alias);
    }
    (names, preferred)
}
/// The `(alias, version)` an add selector asks a catalog to move to, or `None`
/// when the catalog already covers it or the selector names no exact version.
pub(super) fn catalog_version_request(
    selector: &str,
    manifest: &PackageManifest,
    catalogs: &Catalogs,
    lockfile: Option<&Lockfile>,
    save_catalog_name: Option<&str>,
) -> Option<(String, String)> {
    let parsed = pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(selector);
    let (Some(alias), Some(wanted)) = (parsed.alias, parsed.bare_specifier) else {
        return None;
    };
    node_semver::Version::parse(&wanted).ok()?;
    let previous = manifest
        .dependencies(DIRECT_GROUPS)
        .find_map(|(name, specifier)| (name == alias).then_some(specifier));
    let catalog_name = crate::per_dep_catalog_name(previous, save_catalog_name);
    let entry = catalogs.get(catalog_name).and_then(|catalog| catalog.get(&alias))?;
    if !crate::catalog_covers(entry, &wanted) {
        return None;
    }
    let resolved = lockfile
        .and_then(|lockfile| lockfile.catalogs.as_ref())
        .and_then(|catalogs| catalogs.get(catalog_name))
        .and_then(|catalog| catalog.get(&alias))
        .map(|entry| entry.version.as_str());
    if resolved == Some(wanted.as_str()) {
        return None;
    }
    Some((alias, wanted))
}
pub(super) struct AddCatalogCtx {
    pub(super) catalogs: Catalogs,
    pub(super) workspace_dir: PathBuf,
    prefix: String,
}
pub(super) struct SelectedAddPreparation {
    pub(super) catalogs: Catalogs,
    pub(super) updated_catalogs: Catalogs,
    pub(super) catalogs_override: Option<Catalogs>,
    pub(super) workspace_dir: PathBuf,
}
pub(super) async fn prepare_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
    add: AddView<'_>,
    owned: &AddOwned,
) -> Result<SelectedAddPreparation, AddError> {
    let first_index = *selected_indices.first().expect("selected add requires a project");
    let catalog_ctx = read_catalog_ctx(&projects[first_index].manifest, add.config)?;
    let mut catalogs = catalog_ctx.catalogs;
    let mut updated_catalogs = Catalogs::new();
    // One resolution across every selected
    // project: the picker is created on first use (a selection that resolves
    // no `latest` tag never builds one), and the shared caches keep the same
    // package from being fetched once per project.
    let resolution = AddResolution::new();
    // Indexed once, before the loop mutates any manifest: a
    // `workspace:` request saved into project A resolves against the
    // versions the projects declared on entry, not against a sibling
    // that this same command already rewrote.
    let workspace_packages = crate::install::build_workspace_packages_map(Some(projects));
    let git_source_cache = Arc::new(pnpm_git_fetcher::GitSourceCache::default());

    for &index in selected_indices {
        let updates = prepare_manifest::<Reporter>(
            &mut projects[index].manifest,
            &AddResolveInputs {
                add,
                http_client_arc: &owned.http_client_arc,
                git_source_cache: &git_source_cache,
                resolution: &resolution,
                save_catalog_name: owned.save_catalog_name.as_deref(),
                catalogs: &catalogs,
                prefix: &catalog_ctx.prefix,
                workspace_packages: workspace_packages.as_ref(),
            },
            owned.dependency_groups.as_deref(),
        )
        .await?;
        merge_catalogs(&mut catalogs, &updates);
        merge_catalogs(&mut updated_catalogs, &updates);
    }

    let catalogs_override = (!updated_catalogs.is_empty()).then_some(catalogs.clone());
    Ok(SelectedAddPreparation {
        catalogs,
        updated_catalogs,
        catalogs_override,
        workspace_dir: catalog_ctx.workspace_dir,
    })
}
/// The manifest group `name` already occupies, in pnpm's
/// `guessDependencyType` scan order.
pub(super) fn guess_dependency_group(
    manifest: &PackageManifest,
    name: &str,
) -> Option<DependencyGroup> {
    [DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Peer]
        .into_iter()
        .find(|&group| manifest.dependencies([group]).any(|(dep, _)| dep == name))
}
/// Resolve every selector against `catalogs` concurrently, then apply them
/// to `manifest`.
///
/// Every selector is resolved before the manifest is touched so the
/// `initial` manifest event still reports the pre-add shape exactly once,
/// however many selectors a single `add` carries. `FuturesOrdered` overlaps
/// the registry requests while keeping the buffered catalog warnings and the
/// applied dependencies in selector order. The resolution state is
/// threaded in so one selected-add pass shares it across every project it
/// touches.
pub(super) async fn prepare_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
    dependency_groups: Option<&[DependencyGroup]>,
) -> Result<Catalogs, AddError> {
    let resolved_dependencies = {
        let mut resolution_futures = FuturesOrdered::new();
        for package_selector in inputs.add.package_names {
            resolution_futures.push_back(resolve_added_dependency(
                package_selector,
                manifest,
                inputs,
            ));
        }
        let mut dependencies = Vec::with_capacity(inputs.add.package_names.len());
        while let Some(result) = resolution_futures.next().await {
            let dependency = result?;
            if let Some(warning) = &dependency.warning {
                Reporter::emit(warning);
            }
            dependencies.push(dependency);
        }
        dependencies
    };

    emit_initial_package_manifest::<Reporter>(manifest);

    for dependency in &resolved_dependencies {
        let groups =
            target_dependency_groups(dependency_groups, manifest, &dependency.package_name);
        for dependency_group in groups {
            manifest
                .add_dependency(
                    &dependency.package_name,
                    &dependency.manifest_specifier,
                    dependency_group,
                )
                .map_err(AddError::AddDependencyToManifest)?;
        }
    }

    let mut updated_catalogs = Catalogs::new();
    for dependency in resolved_dependencies {
        merge_catalogs(&mut updated_catalogs, &dependency.updated_catalogs);
    }
    Ok(updated_catalogs)
}
/// The manifest groups an added dependency is written to. With none requested
/// this is pnpm's `guessDependencyType`: keep an already-declared package in
/// its group; a peer-only entry stays untouched (the install still resolves
/// it); a new package lands in `dependencies`.
pub(super) fn target_dependency_groups(
    dependency_groups: Option<&[DependencyGroup]>,
    manifest: &PackageManifest,
    package_name: &str,
) -> Vec<DependencyGroup> {
    if let Some(groups) = dependency_groups {
        return groups.to_vec();
    }
    match guess_dependency_group(manifest, package_name) {
        Some(DependencyGroup::Peer) => Vec::new(),
        Some(group) => vec![group],
        None => vec![DependencyGroup::Prod],
    }
}
pub(super) fn read_catalog_ctx(
    manifest: &PackageManifest,
    config: &Config,
) -> Result<AddCatalogCtx, AddError> {
    let manifest_dir =
        manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let workspace_dir_opt =
        pnpm_workspace::find_workspace_dir(&manifest_dir).map_err(AddError::FindWorkspaceDir)?;
    let catalogs = if let Some(catalogs) = config.catalogs.clone() {
        catalogs
    } else {
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pnpm_workspace::read_workspace_manifest(dir)
                .map_err(AddError::ReadWorkspaceManifest)?,
            None => None,
        };
        get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
            .map_err(AddError::InvalidCatalogsConfiguration)?
    };
    let workspace_dir = workspace_dir_opt.unwrap_or(manifest_dir);
    let prefix = workspace_dir.to_string_lossy().into_owned();
    Ok(AddCatalogCtx { catalogs, workspace_dir, prefix })
}
pub(super) fn merge_catalogs(target: &mut Catalogs, updates: &Catalogs) {
    for (catalog_name, entries) in updates {
        let catalog = target.entry(catalog_name.clone()).or_default();
        for (dependency, specifier) in entries {
            catalog.insert(dependency.clone(), specifier.clone());
        }
    }
}
pub(super) fn persist_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
) -> Result<(), AddError> {
    for &index in selected_indices {
        persist_manifest::<Reporter>(&mut projects[index].manifest)?;
    }
    Ok(())
}
pub(super) fn persist_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
) -> Result<(), AddError> {
    let updated = manifest.save_and_get_written_value().map_err(AddError::SaveManifest)?;
    let prefix = package_manifest_prefix(manifest);
    Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix, updated },
    }));
    Ok(())
}
/// Write an added dependency's catalog entry when the decision moves it into a
/// catalog, and hand back the specifier the manifest records.
pub(super) fn apply_catalog_decision(
    decision: CatalogDecision,
    package_name: &str,
    bare_specifier: String,
    updated_catalogs: &mut Catalogs,
) -> String {
    match decision {
        CatalogDecision::KeepDirect => bare_specifier,
        CatalogDecision::Catalog { manifest_specifier, updated_entry } => {
            if let Some(entry) = updated_entry {
                updated_catalogs
                    .entry(entry.catalog_name)
                    .or_default()
                    .insert(package_name.to_string(), entry.specifier);
            }
            manifest_specifier
        }
    }
}
