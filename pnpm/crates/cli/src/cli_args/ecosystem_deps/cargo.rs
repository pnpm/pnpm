use super::{
    EcosystemUpdateStepOptions,
    cargo_manifest_ops::{
        DeclaredCargoDep, apply_manifest_edits, declared_cargo_deps, find_cargo_workspace_root,
        format_cargo_saved_spec, read_locked_cargo_versions, select_locked_version,
    },
    selectors::CargoUpdateSelector,
};
use crate::{
    cargo_deps::{
        self, CargoLockfilePolicy, cargo_auth_headers, checkout, metadata_paths,
        provision_toolchain, resolve_target_version, workspace_root,
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
use semver::{Version, VersionReq};
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
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    let cargo_root = find_cargo_workspace_root(project_dir, workspace_limit);
    let cargo_lock_path = cargo_root.join("Cargo.lock");
    if !cargo_lock_path.is_file() {
        return Ok(Vec::new());
    }
    let locked = read_locked_cargo_versions(&cargo_lock_path)?;
    let auth_headers = cargo_auth_headers(config)?;
    let mut outdated = Vec::new();
    for dep in declared {
        if !query.include_direct.contains(&dep.group) || !query.matches_name(&dep.alias) {
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
    Ok(Some(build_cargo_outdated_package(dep, current, &target, &wanted, workspace_name)))
}

fn build_cargo_outdated_package(
    dep: &DeclaredCargoDep,
    current: &Version,
    target: &Version,
    wanted: &Version,
    workspace_name: Option<&str>,
) -> OutdatedPackage {
    let homepage = pnpm_cargo_resolver::is_crates_io(pnpm_cargo_resolver::CRATES_IO_SPARSE_INDEX)
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
    if !config.cargo.enabled {
        return Ok(false);
    }
    let manifest_path = project_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        validate_no_explicit_cargo_requests(options.requests)?;
        return Ok(false);
    }
    let declared = declared_cargo_deps(project_dir, config.workspace_dir.as_deref())?;
    validate_explicit_cargo_requests_exist(options.requests, &declared)?;
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

fn validate_no_explicit_cargo_requests(requests: &[CargoUpdateSelector]) -> Result<()> {
    if let Some(req) = requests.iter().find(|req| req.explicit) {
        bail!("crate {} is not a declared dependency in Cargo.toml", req.name);
    }
    Ok(())
}

fn validate_explicit_cargo_requests_exist(
    requests: &[CargoUpdateSelector],
    declared: &[DeclaredCargoDep],
) -> Result<()> {
    for req in requests.iter().filter(|req| req.explicit) {
        if !declared
            .iter()
            .any(|dep| dep.alias == req.name || dep.package_name == req.name)
        {
            bail!("crate {} is not a declared dependency in Cargo.toml", req.name);
        }
    }
    Ok(())
}

fn select_cargo_deps_to_update<'a>(
    declared: &'a [DeclaredCargoDep],
    options: &'a CargoUpdateOptions<'a>,
) -> Vec<(&'a DeclaredCargoDep, Option<&'a str>)> {
    declared
        .iter()
        .filter_map(|dep| match_cargo_dep(dep, options).map(|spec| (dep, spec)))
        .collect()
}

fn match_cargo_dep<'a>(
    dep: &DeclaredCargoDep,
    options: &'a CargoUpdateOptions<'a>,
) -> Option<Option<&'a str>> {
    if !options.step.include_groups.contains(&dep.group)
        || options.step.ignore_matcher.is_some_and(|matcher| matcher.matches(&dep.alias))
    {
        return None;
    }
    if !options.restrict_to_selectors {
        return Some(None);
    }
    if let Some(req) = options.requests
        .iter()
        .find(|req| req.name == dep.alias || req.name == dep.package_name)
    {
        return Some(req.version_spec.as_deref());
    }
    options.bare_matcher
        .is_some_and(|matcher| matcher.matches(&dep.alias))
        .then_some(None)
}

async fn execute_cargo_update<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    manifest_path: &Path,
    declared: &[DeclaredCargoDep],
    selected: &[(&DeclaredCargoDep, Option<&str>)],
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
    let cargo_root = workspace_root(manifest_path, checkout_dir.as_deref()).await?;
    let locked = read_locked_cargo_versions(&cargo_root.join("Cargo.lock")).unwrap_or_default();
    let (staged_edits, final_edits) =
        build_cargo_update_edits(declared, selected, &locked, &context, options).await?;
    let originals = snapshot_cargo_manifests(declared)?;
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

async fn build_cargo_update_edits(
    declared: &[DeclaredCargoDep],
    selected: &[(&DeclaredCargoDep, Option<&str>)],
    locked: &HashMap<String, Vec<Version>>,
    context: &InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<(Vec<ManifestEdit>, Vec<ManifestEdit>)> {
    let auth_headers = cargo_auth_headers(context.config)?;
    let mut staged = Vec::new();
    let mut finalized = Vec::new();
    for (dep, explicit_spec) in selected {
        let (staged_edit, final_edit) =
            resolve_selected_cargo_edit(dep, *explicit_spec, &auth_headers, context, options)
                .await?;
        staged.push(staged_edit);
        finalized.push(final_edit);
    }
    for dep in declared {
        if selected
            .iter()
            .any(|(sel, _)| sel.alias == dep.alias && sel.table == dep.table)
        {
            continue;
        }
        if let Some(locked_ver) = select_locked_version(locked, dep) {
            staged.push((
                dep.manifest_path.clone(),
                dep.table,
                dep.alias.clone(),
                format!("={locked_ver}"),
            ));
        }
    }
    Ok((staged, finalized))
}

async fn resolve_selected_cargo_edit(
    dep: &DeclaredCargoDep,
    explicit_spec: Option<&str>,
    auth_headers: &pnpm_network::AuthHeaders,
    context: &InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<(ManifestEdit, ManifestEdit)> {
    let target_req = match explicit_spec {
        Some(spec) => Some(VersionReq::parse(spec).into_diagnostic()?),
        None if options.step.latest => None,
        None => Some(dep.version_req.clone()),
    };
    let resolved = resolve_target_version(
        context.config,
        auth_headers,
        &dep.package_name,
        target_req.as_ref(),
        &context.http_client,
    )
    .await?
    .ok_or_else(|| miette::miette!("no matching version found for crate {}", dep.package_name))?;
    let saved_spec = format_cargo_saved_spec(
        &dep.raw_spec,
        &resolved,
        explicit_spec,
        options.step.save_exact,
        options.step.save_prefix,
    )?;
    let staged = (dep.manifest_path.clone(), dep.table, dep.alias.clone(), format!("={resolved}"));
    let finalized = (dep.manifest_path.clone(), dep.table, dep.alias.clone(), saved_spec);
    Ok((staged, finalized))
}

fn snapshot_cargo_manifests(declared: &[DeclaredCargoDep]) -> Result<BTreeMap<PathBuf, String>> {
    let mut originals = BTreeMap::new();
    for dep in declared {
        if !originals.contains_key(&dep.manifest_path) && dep.manifest_path.is_file() {
            let contents = fs::read_to_string(&dep.manifest_path)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", dep.manifest_path.display()))?;
            originals.insert(dep.manifest_path.clone(), contents);
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
