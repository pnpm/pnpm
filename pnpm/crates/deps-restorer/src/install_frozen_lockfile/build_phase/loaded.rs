use super::{BuildPhaseError, BuildPhaseInputs, post_build_bins};

pub(super) fn finish_cas_bin_links(
    inputs: &BuildPhaseInputs<'_>,
    build_output: &crate::BuildModulesOutput,
    bin_state: &crate::build_options::BuildBinState,
) -> Result<(), BuildPhaseError> {
    let config = inputs.policy.config;
    let selected = post_build_bins::loaded_snapshots(inputs);
    let native_inputs = BuildPhaseInputs {
        graph: crate::BuildPhaseGraph { snapshots: Some(&selected), ..inputs.graph },
        ..*inputs
    };
    super::finish_dependency_bin_links(&native_inputs, build_output, bin_state)?;
    if build_output.mutated_slots {
        crate::cas::refresh_bins(&crate::cas::BinInstall {
            config,
            root: inputs.directories.workspace_root,
            importers: inputs.graph.importers,
            trusted_importer_ids: inputs.graph.trusted_importer_ids,
        })
        .map_err(BuildPhaseError::CasBins)?;
    }
    Ok(())
}
