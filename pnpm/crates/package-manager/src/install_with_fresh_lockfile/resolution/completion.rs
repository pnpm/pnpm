use super::{
    FreshInputs,
    HashSet,
    InstallWithFreshLockfileError,
    PatchUsageScope,
    Reporter,
    ResolutionPrep,
    ResolvePass,
    Resolved,
    check_patch_usage,
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
    crate::minimum_release_age::handle_minimum_release_age_violations::<Reporter>(
        install.drivers.config,
        install.projects.lockfile_dir,
        &workspace_result.merged_tree.policy_violations,
        can_prompt_now,
        policy_excludes_now,
    )
    .await
    .map_err(InstallWithFreshLockfileError::MinimumReleaseAge)?;
    check_patch_usage::<Reporter>(
        install.drivers.config,
        prep.patches.record.as_deref(),
        &workspace_result.merged_tree.applied_patches,
        PatchUsageScope {
            real_importer_ids: install.projects.real_ids,
            selected_importer_ids: install.projects.selected_ids,
            merge_wanted_lockfile: install.lockfiles.merge_wanted,
        },
    )?;
    Ok(())
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
        reuse: crate::install_with_fresh_lockfile::resolution::ResolutionReuseGuard {
            guard_previous_importers: install.lockfiles.merge_wanted
                .filter(|_| install.drivers.config.dedupe_injected_deps)
                .map(|lockfile| &lockfile.importers),
            guard_update_reuse_scope: prep.reuse.scope,
            guard_update_reuse_scopes_by_importer: prep.reuse.by_importer,
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
