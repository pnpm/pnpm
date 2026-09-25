use super::{
    Config, Context, Host, InstallArgs, Path, PathBuf, ReporterType, read_manifest_json,
    reporter_emit, resolve_bool_override, warn_deprecated_override_version_references,
    warn_ignored_pnpm_manifest_fields, warn_unapplied_package_configs,
    warn_unmatched_registry_options,
};
use crate::cli_args::yarn_workspaces_field::{
    create_workspace_yaml_from_yarn_workspaces, warn_about_workspaces_field,
};

/// [`select_workspace_projects`](super::select_workspace_projects), optionally running the install's
/// workspace-cycle search over the selection graph while it is still in
/// hand. Callers pass `true` only for a run that is certain to reach
/// the cycle check — the "Already up to date" fast path returns before
/// it, and a search it never reads would tax exactly that path.
/// `runtimeOnFail` is a workspace-level override the projects carry into
/// their own installs, so it is applied to each manifest as it is read.
pub(super) fn apply_runtime_on_fail(cfg: &Config, projects: &mut [pnpm_workspace::Project]) {
    let Some(runtime_on_fail) = cfg.runtime_on_fail else {
        return;
    };
    for project in projects {
        pnpm_package_manifest::apply_runtime_on_fail_override(
            project.manifest.value_mut(),
            runtime_on_fail.as_str(),
        );
    }
}

/// Shared workspace-root and package-manager policy derivation used by the
/// install, dedupe, and prune dispatch paths.
pub(crate) fn derive_config_root(
    cfg: &mut Config,
    dir_ref: &Path,
    reporter: ReporterType,
) -> miette::Result<PathBuf> {
    let (config_root, root_manifest) = read_config_root_manifest(cfg, dir_ref)?;
    // pnpm warns from config-reading, so the notice lands ahead of any
    // install output. This is the install family's earliest point that
    // knows the root manifest's directory.
    warn_ignored_pnpm_manifest_fields(root_manifest.as_ref());
    create_workspace_yaml_from_yarn_workspaces(cfg, dir_ref, root_manifest.as_ref())?;
    warn_about_config_settings(cfg, reporter);
    Ok(config_root)
}

/// The config warnings of [`derive_config_root`] for a command that writes
/// a lockfile but never creates `pnpm-workspace.yaml`, such as `import`.
pub(crate) fn warn_about_config_root(
    cfg: &Config,
    dir_ref: &Path,
    reporter: ReporterType,
) -> miette::Result<()> {
    let (_, root_manifest) = read_config_root_manifest(cfg, dir_ref)?;
    warn_ignored_pnpm_manifest_fields(root_manifest.as_ref());
    warn_about_workspaces_field(cfg, root_manifest.as_ref());
    warn_about_config_settings(cfg, reporter);
    Ok(())
}

fn read_config_root_manifest(
    cfg: &Config,
    dir_ref: &Path,
) -> miette::Result<(PathBuf, Option<serde_json::Value>)> {
    let config_root = cfg.root_project_manifest_dir(dir_ref).to_path_buf();
    let root_manifest = read_manifest_json(&config_root.join("package.json"))
        .wrap_err("read package manager policy")?;
    Ok((config_root, root_manifest))
}

fn warn_about_config_settings(cfg: &Config, reporter: ReporterType) {
    warn_deprecated_override_version_references(cfg, reporter_emit(reporter));
    warn_unmatched_registry_options(cfg);
    warn_unapplied_package_configs(cfg);
}

fn apply_install_materialization_config(
    cfg: &mut Config,
    materialization: &crate::cli_args::install::InstallMaterializationArgs,
) {
    if materialization.frozen_store || materialization.no_frozen_store {
        cfg.cli_settings.insert("frozenStore".to_string());
    }
    cfg.frozen_store = resolve_bool_override(
        materialization.frozen_store,
        materialization.no_frozen_store,
        cfg.frozen_store,
    );
    if materialization.force {
        cfg.cli_settings.insert("force".to_string());
    }
    cfg.force = materialization.force || cfg.force;
}

fn apply_install_fetching_config(
    cfg: &mut Config,
    fetching: &crate::cli_args::install::InstallFetchArgs,
) {
    if let Some(network_concurrency) = fetching.concurrency {
        cfg.cli_settings.insert("networkConcurrency".to_string());
        cfg.network_concurrency = network_concurrency;
    }
    if let Some(fetch_timeout) = fetching.timeout {
        cfg.cli_settings.insert("fetchTimeout".to_string());
        cfg.fetch_timeout = fetch_timeout;
    }
    if let Some(fetch_warn_timeout_ms) = fetching.warn_timeout_ms {
        cfg.cli_settings.insert("fetchWarnTimeoutMs".to_string());
        cfg.fetch_warn_timeout_ms = fetch_warn_timeout_ms;
    }
    if let Some(fetch_min_speed_ki_bps) = fetching.min_speed_ki_bps {
        cfg.cli_settings.insert("fetchMinSpeedKiBps".to_string());
        cfg.fetch_min_speed_ki_bps = fetch_min_speed_ki_bps;
    }
    if let Some(user_agent) = fetching.user_agent.clone() {
        cfg.cli_settings.insert("userAgent".to_string());
        cfg.user_agent = user_agent;
    }
    if let Some(pnpr_server) = fetching.pnpr_server.clone() {
        cfg.cli_settings.insert("pnprServer".to_string());
        cfg.pnpr_server = Some(pnpr_server);
    }
}

fn apply_install_git_branch_lockfiles_config(
    cfg: &mut Config,
    lockfile_updates: &crate::cli_args::install::LockfileUpdateArgs,
) {
    if lockfile_updates.merge_git_branch_lockfiles {
        cfg.cli_settings.insert("mergeGitBranchLockfiles".to_string());
        cfg.merge_git_branch_lockfiles = true;
    } else if !lockfile_updates.merge_git_branch_lockfiles_branch_pattern.is_empty() {
        cfg.cli_settings.insert("mergeGitBranchLockfilesBranchPattern".to_string());
        cfg.merge_git_branch_lockfiles_branch_pattern.clone_from(
            &lockfile_updates.merge_git_branch_lockfiles_branch_pattern,
        );
        cfg.apply_git_branch_lockfile_derivation::<Host>();
    }
}

pub(crate) fn apply_install_cli_config(cfg: &mut Config, args: &InstallArgs) {
    args.network_cache.apply(cfg);
    args.lockfile_updates.dedupe.apply(cfg);
    args.scripts.apply(cfg);
    apply_install_materialization_config(cfg, &args.materialization);
    apply_install_fetching_config(cfg, &args.fetching);
    apply_install_git_branch_lockfiles_config(cfg, &args.lockfile_updates);
}

/// Whether the active directory has no manifest of its own and is none of
/// the workspace's projects, so the manifest at hand stands in for one.
pub(super) fn active_manifest_is_standin(
    active_dir: &Path,
    projects: &[pnpm_workspace::Project],
) -> miette::Result<bool> {
    let normalized_active_dir = pnpm_fs::lexical_normalize(active_dir);
    Ok(!active_dir.join("package.json").is_file()
        && pnpm_workspace::try_read_project_manifest(active_dir)
            .map_err(miette::Report::new)?
            .is_none()
        && !projects
            .iter()
            .any(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_active_dir))
}
