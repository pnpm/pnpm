use super::{
    EcosystemUpdateStepOptions,
    cargo_manifest_ops::{
        DeclaredCargoDep, apply_manifest_edits, declared_cargo_deps, find_cargo_workspace_root,
        format_cargo_saved_spec, read_locked_cargo_versions, same_declared_dep,
        select_locked_version,
    },
    cargo_selection::{
        SelectedCargoDep, SelectedCargoTarget, admits_enabled_features, is_held_as_downgrade,
        is_held_by_patch, select_cargo_deps_to_update, target_requirement,
    },
    cargo_workspace_scope::{CargoWorkspaceScope, resolve_cargo_workspace_scope},
    selectors::CargoUpdateSelector,
};
use crate::{
    cargo_deps::{
        self, CargoLockfilePolicy, cargo_auth_headers, checkout, fetch_crate_index_file,
        metadata_paths, provision_toolchain, resolve_target_version,
    },
    cli_args::outdated::{
        OutdatedPackage, OutdatedQuery, TargetVersion,
        query::{OutdatedDisplayVersions, OutdatedMetadata},
    },
    ecosystem_install::InstallContext,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_config::Config;
use pnpm_install_coordinator::{InstallPlan, InstallTask};
use pnpm_matcher::Matcher;
use pnpm_network::ThrottledClient;
use pnpm_reporter::Reporter;
use semver::Version;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct CargoUpdateOptions<'a> {
    pub(crate) requests: &'a [CargoUpdateSelector],
    pub(crate) bare_matcher: Option<&'a Matcher>,
    pub(crate) restrict_to_selectors: bool,
    pub(crate) step: &'a EcosystemUpdateStepOptions<'a>,
}

pub(crate) fn has_cargo_lockfile(project_dir: &Path, workspace_limit: Option<&Path>) -> bool {
    find_cargo_workspace_root(project_dir, workspace_limit).join("Cargo.lock").is_file()
}

pub(crate) async fn collect_cargo_outdated(
    project_dir: &Path,
    workspace_name: Option<String>,
    query: &OutdatedQuery<'_>,
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<Vec<OutdatedPackage>> {
    if !config.cargo.enabled {
        return Ok(Vec::new());
    }
    let workspace_limit = config.workspace_dir.as_deref();
    let declared = declared_cargo_deps(project_dir, workspace_limit)?;
    let cargo_lock_path =
        find_cargo_workspace_root(project_dir, workspace_limit).join("Cargo.lock");
    if declared.is_empty() || !cargo_lock_path.is_file() {
        return Ok(Vec::new());
    }
    let locked = read_locked_cargo_versions(&cargo_lock_path)?;
    let auth_headers = cargo_auth_headers(config)?;
    let mut outdated = Vec::new();
    for dep in declared {
        if !query.include_direct.contains(&dep.group)
            || !(query.matches_name(&dep.alias) || query.matches_name(&dep.package_name))
        {
            continue;
        }
        if let Some(pkg) = resolve_outdated_cargo_dep(
            &dep,
            &locked,
            workspace_name.as_deref(),
            query,
            config,
            &auth_headers,
            http_client,
        )
        .await?
        {
            outdated.push(pkg);
        }
    }
    Ok(outdated)
}

async fn resolve_outdated_cargo_dep(
    dep: &DeclaredCargoDep,
    locked: &HashMap<String, Vec<Version>>,
    workspace_name: Option<&str>,
    query: &OutdatedQuery<'_>,
    config: &Config,
    auth_headers: &pnpm_network::AuthHeaders,
    http_client: &ThrottledClient,
) -> Result<Option<OutdatedPackage>> {
    let Some(current) = select_locked_version(locked, dep) else {
        return Ok(None);
    };
    let wanted = resolve_target_version(
        config,
        auth_headers,
        &dep.package_name,
        Some(&dep.version_req),
        http_client,
    )
    .await?
    .unwrap_or_else(|| current.clone());
    let target = match query.target_version {
        TargetVersion::WithinRange => wanted.clone(),
        TargetVersion::Latest => {
            let Some(latest) =
                resolve_target_version(config, auth_headers, &dep.package_name, None, http_client)
                    .await?
            else {
                return Ok(None);
            };
            latest
        }
    };
    if target <= *current {
        return Ok(None);
    }
    Ok(Some(build_cargo_outdated_package(
        dep,
        (current, &target, &wanted),
        workspace_name,
        config.cargo_index_url(),
    )))
}

fn build_cargo_outdated_package(
    dep: &DeclaredCargoDep,
    (current, target, wanted): (&Version, &Version, &Version),
    workspace_name: Option<&str>,
    index_url: &str,
) -> OutdatedPackage {
    let homepage = pnpm_cargo_resolver::is_crates_io(index_url)
        .then(|| format!("https://crates.io/crates/{}", dep.package_name));
    OutdatedPackage {
        alias: dep.alias.clone(),
        package_name: dep.package_name.clone(),
        belongs_to: dep.group,
        current: semver_to_node_version(current),
        target: semver_to_node_version(target),
        wanted: semver_to_node_version(wanted),
        github_action: false,
        metadata: OutdatedMetadata {
            deprecated: None,
            homepage,
            workspace: workspace_name.map(str::to_string),
            display_versions: Some(Box::new(OutdatedDisplayVersions {
                current: current.to_string(),
                target: target.to_string(),
                wanted: wanted.to_string(),
            })),
        },
    }
}

fn semver_to_node_version(version: &Version) -> node_semver::Version {
    version
        .to_string()
        .parse()
        .unwrap_or_else(|_| node_semver::Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            build: Vec::new(),
            pre_release: Vec::new(),
        })
}

pub(crate) async fn update_cargo_project<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    context: InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<bool> {
    let config = context.config;
    let manifest_path = project_dir.join("Cargo.toml");
    if !config.cargo.enabled || !manifest_path.is_file() {
        return Ok(false);
    }
    let declared = declared_cargo_deps(project_dir, config.workspace_dir.as_deref())?;
    let selected = select_cargo_deps_to_update(&declared, options);
    if selected.is_empty() {
        return Ok(false);
    }
    execute_cargo_update::<Reporter>(
        project_dir,
        &manifest_path,
        &declared,
        &selected,
        context,
        options,
    )
    .await?;
    Ok(true)
}

pub(crate) fn validate_explicit_cargo_requests(
    project_dirs: &[PathBuf],
    workspace_root: Option<&Path>,
    requests: &[CargoUpdateSelector],
) -> Result<()> {
    let explicit: Vec<&CargoUpdateSelector> = requests
        .iter()
        .filter(|req| req.explicit)
        .collect();
    if explicit.is_empty() {
        return Ok(());
    }
    let mut all_declared = Vec::new();
    for dir in project_dirs {
        if dir.join("Cargo.toml").is_file() {
            all_declared.extend(declared_cargo_deps(dir, workspace_root)?);
        }
    }
    for req in explicit {
        if !all_declared
            .iter()
            .any(|dep| dep.alias == req.name || dep.package_name == req.name)
        {
            bail!("crate {} is not a declared dependency in Cargo.toml", req.name);
        }
    }
    Ok(())
}

async fn execute_cargo_update<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    manifest_path: &Path,
    declared: &[DeclaredCargoDep],
    selected: &[SelectedCargoDep<'_>],
    context: InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<()> {
    let config = context.config;
    let checkout_dir = checkout(config.workspace_dir.as_deref().unwrap_or(project_dir));
    provision_toolchain::<Reporter>(
        config,
        &context.http_client,
        std::slice::from_ref(&manifest_path.to_path_buf()),
        checkout_dir.as_deref(),
    )
    .await?;
    let scope =
        resolve_cargo_workspace_scope(manifest_path, declared, checkout_dir.as_deref()).await?;
    let (staged_edits, final_edits) =
        build_cargo_update_edits::<Reporter>(&scope, selected, &context, options).await?;
    let cargo_root = scope.root;
    let originals = snapshot_cargo_manifests(&staged_edits)?;
    if let Err(err) = apply_manifest_edits(&staged_edits) {
        let _ = restore_cargo_manifests(&originals);
        return Err(err);
    }
    let metadata = metadata_paths(&cargo_root).to_vec();
    let task = InstallTask::new(
        metadata,
        cargo_deps::prepare::<Reporter>(
            context,
            vec![cargo_root.clone()],
            checkout_dir,
            CargoLockfilePolicy::Resolve,
        ),
    );
    let run_res = InstallPlan::new(cargo_root).with_task(task).run().await;
    restore_cargo_manifests(&originals)?;
    run_res?;
    if !options.step.no_save {
        apply_manifest_edits(&final_edits)?;
    }
    Ok(())
}

type ManifestEdit = (PathBuf, &'static str, String, String);

/// The manifest edits that stage the update for Cargo and the ones saved
/// after it.
async fn build_cargo_update_edits<Reporter: self::Reporter>(
    scope: &CargoWorkspaceScope,
    selected: &[SelectedCargoDep<'_>],
    context: &InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<(Vec<ManifestEdit>, Vec<ManifestEdit>)> {
    let auth_headers = cargo_auth_headers(context.config)?;
    let locked = read_locked_cargo_versions(&scope.root.join("Cargo.lock")).unwrap_or_default();
    let mut staged = Vec::new();
    let mut finalized = Vec::new();
    let mut updated = Vec::new();
    for selection in selected {
        let target = SelectedCargoTarget {
            selection,
            scope,
            locked: select_locked_version(&locked, selection.dep),
        };
        let Some((staged_edit, final_edit)) =
            resolve_selected_cargo_edit::<Reporter>(&target, &auth_headers, context, options)
                .await?
        else {
            continue;
        };
        staged.push(staged_edit);
        finalized.push(final_edit);
        updated.push(selection.dep);
    }
    staged.extend(pin_unchanged_deps(scope, &locked, &updated));
    Ok((staged, finalized))
}

/// The edits that pin each dependency the update leaves alone to its
/// locked version while Cargo resolves. A crate `[patch.crates-io]`
/// replaces is left as declared: that requirement is what selects the
/// replacement, and its `Cargo.lock` entry is not a registry version.
fn pin_unchanged_deps(
    scope: &CargoWorkspaceScope,
    locked: &HashMap<String, Vec<Version>>,
    updated: &[&DeclaredCargoDep],
) -> Vec<ManifestEdit> {
    scope.declared
        .iter()
        .filter(|dep| {
            !scope.is_patched(&dep.package_name)
                && !updated.iter().any(|sel| same_declared_dep(sel, dep))
        })
        .filter_map(|dep| {
            let locked_ver = select_locked_version(locked, dep)?;
            Some((
                dep.manifest_path.clone(),
                dep.table,
                dep.alias.clone(),
                format!("={locked_ver}"),
            ))
        })
        .collect()
}

/// The edits that move a selected dependency to its target version, or
/// `None` when the dependency keeps its version: see [`is_held_by_patch`],
/// [`is_held_as_downgrade`], and [`admits_enabled_features`].
async fn resolve_selected_cargo_edit<Reporter: self::Reporter>(
    target: &SelectedCargoTarget<'_>,
    auth_headers: &pnpm_network::AuthHeaders,
    context: &InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<Option<(ManifestEdit, ManifestEdit)>> {
    let dep = target.selection.dep;
    let explicit_spec = target.selection.explicit_spec;
    if is_held_by_patch::<Reporter>(target) {
        return Ok(None);
    }
    let target_req = target_requirement(target, options.step.latest)?;
    let index_file = fetch_crate_index_file(
        context.config,
        auth_headers,
        &dep.package_name,
        &context.http_client,
    )
    .await?;
    let resolved = pnpm_cargo_resolver::latest_matching_version(
        &dep.package_name,
        &index_file,
        target_req.as_ref(),
    )
    .wrap_err_with(|| format!("select a matching version of crate {}", dep.package_name))?
    .ok_or_else(|| miette::miette!("no matching version found for crate {}", dep.package_name))?;
    if is_held_as_downgrade::<Reporter>(target, &resolved)
        || !admits_enabled_features::<Reporter>(target, &index_file, &resolved)?
    {
        return Ok(None);
    }
    let saved_spec = format_cargo_saved_spec(
        &dep.raw_spec,
        &resolved,
        explicit_spec,
        options.step.save_exact,
        options.step.save_prefix,
    )?;
    let staged = (dep.manifest_path.clone(), dep.table, dep.alias.clone(), format!("={resolved}"));
    let finalized = (dep.manifest_path.clone(), dep.table, dep.alias.clone(), saved_spec);
    Ok(Some((staged, finalized)))
}

fn snapshot_cargo_manifests(edits: &[ManifestEdit]) -> Result<BTreeMap<PathBuf, String>> {
    let mut originals = BTreeMap::new();
    for (manifest_path, _, _, _) in edits {
        if !originals.contains_key(manifest_path) && manifest_path.is_file() {
            let contents = fs::read_to_string(manifest_path)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
            originals.insert(manifest_path.clone(), contents);
        }
    }
    Ok(originals)
}

fn restore_cargo_manifests(originals: &BTreeMap<PathBuf, String>) -> Result<()> {
    for (path, contents) in originals {
        pnpm_fs::write_atomic(path, contents.as_bytes()).into_diagnostic()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
