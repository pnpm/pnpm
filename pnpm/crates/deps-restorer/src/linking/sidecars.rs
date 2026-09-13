use super::{LinkPhaseError, LinkPhaseInputs};
use pnpm_config::NodeLinker;

pub(super) fn write_project_sidecars(inputs: &LinkPhaseInputs<'_>) -> Result<(), LinkPhaseError> {
    let config = inputs.ctx.config;
    let phase_start = std::time::Instant::now();
    if crate::should_write_package_map(config, inputs.ctx.linker.kind) {
        crate::package_map::write_package_map(
            inputs.graph.sidecar_lockfile,
            &crate::package_map::PackageMapOptions {
                lockfile_dir: inputs.ctx.workspace_root,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                layout: inputs.ctx.linker.layout,
                project_manifests: inputs.projects.manifests,
            },
        )
        .map_err(LinkPhaseError::WritePackageMap)?;
    } else if inputs.ctx.linker.kind != NodeLinker::Hoisted {
        // A hoisted install writes its map from its own linker, which
        // runs after this one — see `should_write_hoisted_package_map`.
        // Only the linkers whose map this gate speaks for may take one
        // away.
        crate::package_map::remove_package_map(&config.modules_dir);
    }
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "link.package_map",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    if matches!(inputs.ctx.linker.kind, NodeLinker::Pnp) {
        crate::write_pnp_file(
            inputs.graph.sidecar_lockfile,
            inputs.ctx.workspace_root,
            config,
            inputs.ctx.linker.layout,
            inputs.projects.manifests,
        )
        .map_err(LinkPhaseError::WritePnpFile)?;
    }
    Ok(())
}
