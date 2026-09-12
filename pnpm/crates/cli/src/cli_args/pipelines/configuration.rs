use super::{
    Config, Context, Host, InstallArgs, Path, PathBuf, ReporterType, read_manifest_json,
    reporter_emit, resolve_bool_override, warn_deprecated_override_version_references,
    warn_ignored_pnpm_manifest_fields, warn_unapplied_package_configs,
    warn_unmatched_registry_options, warn_unsupported_workspaces_field,
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
    cfg: &Config,
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
    warn_unsupported_workspaces_field(root_manifest.as_ref(), cfg.workspace_dir.as_deref());
    warn_deprecated_override_version_references(cfg, reporter_emit(reporter));
    warn_unmatched_registry_options(cfg);
    warn_unapplied_package_configs(cfg);
    Ok(config_root)
}

pub(crate) fn apply_install_cli_config(cfg: &mut Config, args: &InstallArgs) {
    cfg.offline = resolve_bool_override(args.offline, args.no_offline, cfg.offline);
    cfg.prefer_offline =
        resolve_bool_override(args.prefer_offline, args.no_prefer_offline, cfg.prefer_offline);
    cfg.frozen_store =
        resolve_bool_override(args.frozen_store, args.no_frozen_store, cfg.frozen_store);
    cfg.ignore_scripts =
        resolve_bool_override(args.ignore_scripts, args.no_ignore_scripts, cfg.ignore_scripts);
    cfg.ignore_pnpmfile = args.ignore_pnpmfile || cfg.ignore_pnpmfile;
    cfg.force = args.force || cfg.force;
    if let Some(network_concurrency) = args.network_concurrency {
        cfg.network_concurrency = network_concurrency;
    }
    if let Some(fetch_timeout) = args.fetch_timeout {
        cfg.fetch_timeout = fetch_timeout;
    }
    if let Some(fetch_warn_timeout_ms) = args.fetch_warn_timeout_ms {
        cfg.fetch_warn_timeout_ms = fetch_warn_timeout_ms;
    }
    if let Some(fetch_min_speed_ki_bps) = args.fetch_min_speed_ki_bps {
        cfg.fetch_min_speed_ki_bps = fetch_min_speed_ki_bps;
    }
    if let Some(user_agent) = args.user_agent.clone() {
        cfg.user_agent = user_agent;
    }
    if let Some(pnpr_server) = args.pnpr_server.clone() {
        cfg.pnpr_server = Some(pnpr_server);
    }
    // pnpm merges its CLI options into the config *before* deciding
    // `mergeGitBranchLockfiles`, so a pattern given on the command line
    // still gets matched against the current branch — and an explicit
    // `--merge-git-branch-lockfiles` settles the question without it.
    if args.merge_git_branch_lockfiles {
        cfg.merge_git_branch_lockfiles = true;
    } else if !args.merge_git_branch_lockfiles_branch_pattern.is_empty() {
        cfg.merge_git_branch_lockfiles_branch_pattern
            .clone_from(&args.merge_git_branch_lockfiles_branch_pattern);
        cfg.apply_git_branch_lockfile_derivation::<Host>();
    }
}
