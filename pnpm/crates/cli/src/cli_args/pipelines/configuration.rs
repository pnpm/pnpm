use super::{
    Config, Context, Host, InstallArgs, Path, PathBuf, ReporterType, read_manifest_json,
    reporter_emit, resolve_bool_override, warn_deprecated_override_version_references,
    warn_ignored_pnpm_manifest_fields, warn_unapplied_package_configs,
    warn_unmatched_registry_options,
};
use crate::cli_args::yarn_workspaces_field::create_workspace_yaml_from_yarn_workspaces;

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
    let config_root = cfg.root_project_manifest_dir(dir_ref).to_path_buf();
    let root_manifest = read_manifest_json(&config_root.join("package.json"))
        .wrap_err("read package manager policy")?;
    // pnpm warns from config-reading, so the notice lands ahead of any
    // install output. This is the install family's earliest point that
    // knows the root manifest's directory.
    warn_ignored_pnpm_manifest_fields(root_manifest.as_ref());
    create_workspace_yaml_from_yarn_workspaces(cfg, dir_ref, root_manifest.as_ref())?;
    warn_deprecated_override_version_references(cfg, reporter_emit(reporter));
    warn_unmatched_registry_options(cfg);
    warn_unapplied_package_configs(cfg);
    Ok(config_root)
}

pub(crate) fn apply_install_cli_config(cfg: &mut Config, args: &InstallArgs) {
    args.network_cache.apply(cfg);
    args.lockfile_updates.dedupe.apply(cfg);
    cfg.frozen_store = resolve_bool_override(
        args.materialization.frozen_store,
        args.materialization.no_frozen_store,
        cfg.frozen_store,
    );
    args.scripts.apply(cfg);
    cfg.force = args.materialization.force || cfg.force;
    if let Some(network_concurrency) = args.fetching.concurrency {
        cfg.network_concurrency = network_concurrency;
    }
    if let Some(fetch_timeout) = args.fetching.timeout {
        cfg.fetch_timeout = fetch_timeout;
    }
    if let Some(fetch_warn_timeout_ms) = args.fetching.warn_timeout_ms {
        cfg.fetch_warn_timeout_ms = fetch_warn_timeout_ms;
    }
    if let Some(fetch_min_speed_ki_bps) = args.fetching.min_speed_ki_bps {
        cfg.fetch_min_speed_ki_bps = fetch_min_speed_ki_bps;
    }
    if let Some(user_agent) = args.fetching.user_agent.clone() {
        cfg.user_agent = user_agent;
    }
    if let Some(pnpr_server) = args.fetching.pnpr_server.clone() {
        cfg.pnpr_server = Some(pnpr_server);
    }
    // pnpm merges its CLI options into the config *before* deciding
    // `mergeGitBranchLockfiles`, so a pattern given on the command line
    // still gets matched against the current branch — and an explicit
    // `--merge-git-branch-lockfiles` settles the question without it.
    if args.lockfile_updates.merge_git_branch_lockfiles {
        cfg.merge_git_branch_lockfiles = true;
    } else if !args.lockfile_updates.merge_git_branch_lockfiles_branch_pattern.is_empty() {
        cfg.merge_git_branch_lockfiles_branch_pattern.clone_from(
            &args.lockfile_updates.merge_git_branch_lockfiles_branch_pattern,
        );
        cfg.apply_git_branch_lockfile_derivation::<Host>();
    }
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
