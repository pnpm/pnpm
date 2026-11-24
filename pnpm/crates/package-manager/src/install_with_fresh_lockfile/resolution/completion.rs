use super::{
    FreshInputs,
    HashSet,
    InstallWithFreshLockfileError,
    Reporter,
    ResolutionPrep,
    ResolvePass,
    Resolved,
    interactive_policy,
    report_peer_issues,
};

pub(super) async fn enforce_resolution_policies<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    prep: &ResolutionPrep<Reporter>,
    workspace_result: &pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
) -> Result<(), InstallWithFreshLockfileError> {
    let (can_prompt_now, policy_excludes_now) = interactive_policy(
        install.execution.can_prompt,
        install.execution.policy_excludes,
        install.execution.dry_run,
    );
    let target_workspace_dir = install.drivers.config.target_workspace_dir
        .as_deref()
        .unwrap_or(install.projects.lockfile_dir);
    crate::minimum_release_age::handle_minimum_release_age_violations::<Reporter>(
        install.drivers.config,
        target_workspace_dir,
        &workspace_result.merged_tree.policy_violations,
        can_prompt_now,
        policy_excludes_now,
    )
    .await
    .map_err(InstallWithFreshLockfileError::MinimumReleaseAge)?;
    check_patch_usage::<Reporter>(
        install,
        prep.patches.record.as_deref(),
        &workspace_result.merged_tree,
    )
}

fn check_patch_usage<Report: Reporter>(
    install: FreshInputs<'_>,
    patches: Option<&pnpm_patching::PatchGroupRecord>,
    tree: &pnpm_resolving_deps_resolver::ResolvedTree,
) -> Result<(), InstallWithFreshLockfileError> {
    let Some(patches) = patches else { return Ok(()) };
    if !should_verify_patches(install) {
        return Ok(());
    }
    let deployed_names = install.manifests.deploy_hook.then(|| {
        tree.packages
            .keys()
            .filter_map(|id| pnpm_package_name::parse_dep_path_name_version(id))
            .map(|(name, _)| name)
            .filter(|name| patches.contains_key(*name))
            .collect::<HashSet<&str>>()
    });
    let groups = patches
        .iter()
        .filter_map(|(name, group)| {
            deployed_names
                .as_ref()
                .is_none_or(|names| names.contains(name.as_str()))
                .then_some(group)
        });
    if let Some(warning) = pnpm_patching::verify_patches(
        groups,
        &tree.applied_patches,
        install.drivers.config.allow_unused_patches,
    )
    .map_err(InstallWithFreshLockfileError::UnusedPatch)?
    {
        Report::emit(&pnpm_reporter::LogEvent::Global(pnpm_reporter::GlobalLog {
            level: pnpm_reporter::LogLevel::Warn,
            message: warning.to_string(),
        }));
    }
    Ok(())
}

fn should_verify_patches(install: FreshInputs<'_>) -> bool {
    let config = install.drivers.config;
    if !config.shares_one_lockfile() && config.workspace_dir.is_some() {
        return false;
    }
    let Some(selected) = install.projects.selected_ids else { return true };
    !super::super::is_partial_workspace_selection(install.projects.real_ids, Some(selected))
        && install.lockfiles.merge_wanted.is_some_and(|previous| {
            previous.importers.len() == selected.len()
        })
}

/// A finished resolve pass, with what the phase around it decided.
/// Enforce the policies the pass reports against, gather the peer
/// issues, and assemble the phase's output.
pub(super) async fn collect_resolution<'m, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'m>,
    peer_issues_sink: Option<&crate::PeerIssuesSink>,
    prep: ResolutionPrep<Reporter>,
    pass: ResolvePass<'m>,
) -> Result<Resolved<'m, Reporter>, InstallWithFreshLockfileError> {
    let workspace_result = pass.result;
    enforce_resolution_policies::<Reporter>(install, &prep, &workspace_result).await?;
    let peer_issues = &workspace_result.peers.peer_dependency_issues_by_importer;
    let mut peer_issue_importer_ids: HashSet<String> = peer_issues.keys().cloned().collect();
    peer_issue_importer_ids.extend(pass.linked_peer_importers);
    report_peer_issues(peer_issues_sink, peer_issues);
    report_resolve_phase(pass.started, &workspace_result, pass.importer_manifests.len());
    Ok(Resolved {
        early_materializer: prep.early_materializer,
        importer_manifests: pass.importer_manifests,
        fixed_wanted_lockfile: prep.fixed_wanted_lockfile,
        overrides: crate::install_with_fresh_lockfile::resolution::ResolvedOverrides {
            parsed_overrides: prep.transforms.parsed_overrides,
            overrides: prep.transforms.resolved_overrides,
            versions_overrider: prep.transforms.versions_overrider,
        },
        patches: prep.patches,
        hooks: crate::install_with_fresh_lockfile::resolution::ResolvedHooks {
            after_all_resolved_hook: prep.hooks.pnpmfile_hook,
            after_all_resolved_log: prep.hooks.after_all_resolved_log,
        },
        reuse: crate::install_with_fresh_lockfile::resolution::ResolutionReuse {
            full_resolution: pass.full_resolution,
        },
        graph: crate::install_with_fresh_lockfile::resolution::ResolvedGraph {
            peer_issue_importer_ids,
            merged_graph: workspace_result.peers.graph,
            direct_by_importer: workspace_result.peers.direct_dependencies_by_importer,
            time: workspace_result.time,
        },
    })
}
pub(super) fn report_resolve_phase(
    started: std::time::Instant,
    workspace_result: &pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
    importer_count: usize,
) {
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "resolve_workspace",
        elapsed_ms = started.elapsed().as_millis() as u64,
        importers = importer_count,
        nodes = workspace_result.peers.graph.len(),
        "phase complete",
    );
}
