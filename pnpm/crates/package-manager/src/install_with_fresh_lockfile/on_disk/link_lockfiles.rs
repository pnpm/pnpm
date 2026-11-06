use super::OnDiskInputs;

impl<'a> OnDiskInputs<'a> {
    /// The graphs the link phase reconciles. A tree that moved with its
    /// project relinks every slot's bins, not only the materialized ones.
    pub(super) fn link_lockfiles<'b>(
        &self,
        materialized_snapshots: &'b [pnpm_lockfile::PackageKey],
        selected_snapshots: Option<&'b [pnpm_lockfile::PackageKey]>,
    ) -> pnpm_deps_restorer::LinkLockfiles<'b>
    where
        'a: 'b,
    {
        pnpm_deps_restorer::LinkLockfiles {
            lockfile: self.projects.materialization_lockfile,
            current_lockfile: self.install.prior.lockfile,
            materialized_snapshots: (!self.install.prior.relink_every_slot_bin).then_some(
                materialized_snapshots,
            ),
            selected_snapshots,
            sidecar_lockfile: self.projects.materialization_lockfile,
        }
    }
}
