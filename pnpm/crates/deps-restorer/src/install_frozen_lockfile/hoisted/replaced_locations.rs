//! Recorded hoisted locations the linker is about to replace, forgotten
//! before it starts.

use super::{HoistedLinkerError, HoistedLinkerInputs};

/// Remove from `.modules.yaml` every recorded location this install does not
/// reuse as it is, before the linker replaces or removes it. The walker trusts
/// a recorded location, so an install interrupted after this point must not
/// leave a record of the content it was replacing. Writes nothing when every
/// recorded location is reused.
pub(super) fn forget_replaced_hoisted_locations(
    inputs: &HoistedLinkerInputs<'_>,
    walked: &crate::hoisted_dep_graph::LockfileToDepGraphResult,
) -> Result<(), HoistedLinkerError> {
    let Some(recorded) = inputs.prior.current_hoisted_locations else { return Ok(()) };
    let lockfile_dir = inputs.projects.walker_lockfile_dir;
    let is_reused = |dep_path: &str, location: &str| {
        walked.graph
            .get(&lockfile_dir.join(location))
            .is_some_and(|node| node.present && node.package.dep_path.as_str() == dep_path)
    };
    let all_reused = recorded
        .iter()
        .all(|(dep_path, locations)| {
            locations.iter().all(|location| is_reused(dep_path, location))
        });
    if all_reused {
        return Ok(());
    }
    let modules_dir = &inputs.config.modules_dir;
    let Some(mut modules) =
        pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(modules_dir)
            .map_err(HoistedLinkerError::ReadModules)?
    else {
        return Ok(());
    };
    let kept: crate::HoistedLocations = recorded
        .iter()
        .filter_map(|(dep_path, locations)| {
            let reused: Vec<String> = locations
                .iter()
                .filter(|location| is_reused(dep_path, location))
                .cloned()
                .collect();
            (!reused.is_empty()).then(|| (dep_path.clone(), reused))
        })
        .collect();
    modules.hoisted_locations = (!kept.is_empty()).then_some(kept);
    pnpm_modules_yaml::write_modules_manifest::<pnpm_modules_yaml::Host>(modules_dir, modules)
        .map_err(HoistedLinkerError::WriteModules)
}
