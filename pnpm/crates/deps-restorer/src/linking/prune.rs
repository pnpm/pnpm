//! Reconcile what the previous install left in the importers' trees before
//! the link passes write the new one.

use super::{LinkPhaseError, LinkPhaseInputs, hoist_links::PlannedHoist};
use crate::SkippedSnapshots;
use pnpm_reporter::{LogEvent, LogLevel, Reporter, StatsLog, StatsMessage};

pub(super) fn prune_importer_tree<Reporter: self::Reporter>(
    inputs: &LinkPhaseInputs<'_>,
    skipped: &SkippedSnapshots,
    hoist: &PlannedHoist,
) -> Result<(), LinkPhaseError> {
    let config = inputs.ctx.config;
    let current_lockfile = inputs.graph.current_lockfile.unwrap_or(inputs.graph.lockfile);
    let wanted_hoisted = hoist.plan.as_ref().map(|plan| &plan.result.hoisted_dependencies);
    let removed_count = crate::PruneStaleModules {
        config,
        workspace_root: inputs.projects.symlink_root,
        wanted_lockfile: inputs.graph.lockfile,
        current_lockfile,
        prior_hoisted_dependencies: inputs.prior.hoisted_dependencies,
        wanted_hoisted_dependencies: wanted_hoisted,
        linked: crate::LinkedDirectDeps {
            included_groups: inputs.projects.dependency_groups,
            skipped,
        },
        prune_orphans: inputs.prior.prune.orphans,
    }
    .run::<Reporter>()
    .map_err(LinkPhaseError::PruneStaleModules)?;
    if inputs.prior.prune.direct_dependencies {
        crate::prune_extraneous_deps::PruneExtraneousDirectDeps {
            config,
            workspace_root: inputs.projects.symlink_root,
            wanted_lockfile: inputs.graph.lockfile,
            wanted_hoisted_dependencies: wanted_hoisted,
            public_hoist_targets: hoist.public_targets.as_ref(),
            manifests: inputs.projects.manifests,
            included_groups: inputs.projects.dependency_groups,
            selected_importer_ids: inputs.prior.prune.importer_ids,
        }
        .run::<Reporter>()
        .map_err(LinkPhaseError::PruneStaleModules)?;
    }
    Reporter::emit(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Removed {
            prefix: inputs.ctx.requester.to_owned(),
            removed: removed_count,
        },
    }));

    Ok(())
}
