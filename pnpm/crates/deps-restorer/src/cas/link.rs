use crate::{
    LinkVirtualStoreBins, SkippedSnapshots,
    linking::{LinkPhaseError, LinkPhaseInputs, LinkPhaseOutput},
};

pub(crate) fn link_phase(
    inputs: &LinkPhaseInputs<'_>,
    skipped: &SkippedSnapshots,
) -> Result<LinkPhaseOutput, LinkPhaseError> {
    let selected = inputs.ctx
        .select_loaded_snapshots(inputs.graph.lockfile.snapshots.as_ref())
        .expect("loaded linker selection");
    let selected_keys: Vec<_> = selected.keys().cloned().collect();
    LinkVirtualStoreBins {
        layout: inputs.ctx.linker.layout,
        snapshots: Some(selected),
        selected_snapshots: Some(&selected_keys),
        packages: inputs.graph.lockfile.packages.as_ref(),
        package_manifests: inputs.packages.package_manifests,
        skipped,
        link_options: inputs.ctx.linker.bin_options,
    }
    .run()
    .map_err(LinkPhaseError::LinkVirtualStoreBins)?;
    if !inputs.ctx.config.virtual_store_only {
        crate::cas::write_installation(inputs, skipped).map_err(LinkPhaseError::Cas)?;
    }
    Ok(LinkPhaseOutput { hoisted_build_snapshots: Some(selected_keys), ..LinkPhaseOutput::empty() })
}
