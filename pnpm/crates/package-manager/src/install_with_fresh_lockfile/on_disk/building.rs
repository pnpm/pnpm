use super::{OnDiskInputs, OnDiskProjects};
use crate::{
    CreateVirtualStoreOutput, SkippedSnapshots,
    install_with_fresh_lockfile::errors::InstallWithFreshLockfileError,
};
use pnpm_config::{Config, NodeLinker};
use pnpm_reporter::{LogEvent, LogLevel, Reporter as LogReporter};
use std::{collections::HashMap, path::Path, sync::Arc};

fn emit_provider_ignored_scripts<Reporter: LogReporter + 'static>(strict_dep_builds: bool) {
    Reporter::emit(&LogEvent::IgnoredScripts(pnpm_reporter::IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: Vec::new(),
        strict_dep_builds,
    }));
}

pub(super) async fn run_build<Reporter: LogReporter + 'static>(
    inputs: OnDiskInputs<'_>,
    materialized: &CreateVirtualStoreOutput,
    linked: &pnpm_deps_restorer::linking::LinkPhaseOutput,
    skipped: &SkippedSnapshots,
) -> Result<crate::BuildModulesOutput, InstallWithFreshLockfileError> {
    if inputs.ctx.config.package_provider.is_some() {
        emit_provider_ignored_scripts::<Reporter>(inputs.ctx.config.strict_dep_builds);
        drop(inputs.store.store_index_writer);
        return Ok(crate::BuildModulesOutput::default());
    }
    let top_level_bin_root = inputs.symlink_root();
    let OnDiskInputs {
        install,
        ctx,
        deps_requiring_build_sink,
        patched_dependencies,
        store,
        runtime,
        projects,
        ..
    } = inputs;
    let engine_name = settle_engine_name(runtime.deferred_engine_name, runtime.engine_name).await;
    let extra_env = build_extra_env(ctx.config, ctx.linker.kind, ctx.workspace_root);
    publish_deps_requiring_build(
        deps_requiring_build_sink.as_ref(),
        &materialized.requires_build_by_snapshot,
    );
    let built = run_fresh_build_phase::<Reporter>(
        ctx,
        &projects,
        install.projects.dependency_groups,
        &store.store_index_writer,
        patched_dependencies,
        materialized,
        linked,
        engine_name.as_deref(),
        &extra_env,
        top_level_bin_root,
        skipped,
    )?;
    drop(store.store_index_writer);
    Ok(built)
}

#[expect(
    clippy::too_many_arguments,
    reason = "fresh build phase passes decoupled inputs to deps-restorer"
)]
fn run_fresh_build_phase<Reporter: LogReporter + 'static>(
    ctx: &pnpm_deps_restorer::InstallContext<'_>,
    projects: &OnDiskProjects<'_>,
    dependency_groups: &[pnpm_package_manifest::DependencyGroup],
    store_index_writer: &Arc<pnpm_store_dir::StoreIndexWriter>,
    patched_dependencies: Option<&pnpm_patching::PatchGroupRecord>,
    materialized: &CreateVirtualStoreOutput,
    linked: &pnpm_deps_restorer::linking::LinkPhaseOutput,
    engine_name: Option<&str>,
    extra_env: &HashMap<String, String>,
    top_level_bin_root: &Path,
    skipped: &SkippedSnapshots,
) -> Result<crate::BuildModulesOutput, InstallWithFreshLockfileError> {
    crate::install_frozen_lockfile::run_build_phase::<Reporter>(
        &crate::install_frozen_lockfile::BuildPhaseInputs {
            cache: materialized.build_cache(engine_name, store_index_writer),
            directories: build_directories(ctx, linked, top_level_bin_root),
            graph: pnpm_deps_restorer::BuildPhaseGraph {
                snapshots: projects.materialization_lockfile.snapshots.as_ref(),
                packages: projects.materialization_lockfile.packages.as_ref(),
                importers: &projects.materialization_lockfile.importers,
                dependency_groups,
                materialized_snapshots: linked.build_snapshots(
                    &materialized.materialized_snapshots,
                ),
            },
            policy: pnpm_deps_restorer::BuildPhasePolicy {
                config: ctx.config,
                patch_groups: patched_dependencies,
                allow_build_policy: ctx.allow_build_policy,
                rebuild: None,
            },
            extra_env,
            skipped,
            held_back_bins_dirs: &linked.held_back_bins_dirs,
        },
    )
    .map_err(InstallWithFreshLockfileError::BuildPhase)
}

fn build_directories<'a>(
    ctx: &'a pnpm_deps_restorer::InstallContext<'a>,
    linked: &'a pnpm_deps_restorer::linking::LinkPhaseOutput,
    top_level_bin_root: &'a Path,
) -> pnpm_deps_restorer::BuildPhaseDirectories<'a> {
    pnpm_deps_restorer::BuildPhaseDirectories {
        workspace_root: ctx.workspace_root,
        top_level_bin_root,
        layout: ctx.linker.layout,
        hoisted_pkg_roots_by_key: linked.hoisted_pkg_roots_by_key.as_ref(),
        is_hoisted: ctx.is_hoisted(),
        publicly_hoisted_for_post_build: &linked.publicly_hoisted_for_post_build,
        logged_methods: ctx.logged_methods,
        link_options: ctx.linker.bin_options,
    }
}

pub(super) fn build_extra_env(
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
) -> HashMap<String, String> {
    let mut env = config.extra_env.clone();
    if let Some(node_options) = &config.node_options {
        env.insert("NODE_OPTIONS".to_string(), node_options.clone());
    }
    if matches!(node_linker, NodeLinker::Pnp) {
        let node_options = env.get("NODE_OPTIONS").map(String::as_str);
        env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_require_option(
                &workspace_root.join(crate::PNP_FILENAME),
                node_options,
            ),
        );
    }
    if config.node_experimental_package_map && !matches!(node_linker, NodeLinker::Pnp) {
        let package_map_path = config.modules_dir.join(crate::package_map::PACKAGE_MAP_FILENAME);
        let node_options = env.get("NODE_OPTIONS").map(String::as_str);
        env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_package_map_option(&package_map_path, node_options),
        );
    }
    env
}

pub(super) fn publish_deps_requiring_build(
    sink: Option<&crate::DepsRequiringBuildSink>,
    requires_build_by_snapshot: &HashMap<pnpm_lockfile::PackageKey, bool>,
) {
    let Some(sink) = sink else { return };
    let deps_requiring_build = requires_build_by_snapshot
        .iter()
        .filter(|(_, requires_build)| **requires_build)
        .map(|(snapshot_key, _)| snapshot_key.to_string())
        .collect();
    *sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(deps_requiring_build);
}

pub(super) async fn settle_engine_name(
    deferred: Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    engine_name: Option<String>,
) -> Option<String> {
    match deferred {
        Some(deferred) => deferred.handle.await.ok().flatten(),
        None => engine_name,
    }
}
