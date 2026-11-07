use super::resolve::ResolvedEntry;
use pnpm_cmd_shim::{LinkBinsError, LinkBinsOptions};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// Link the bins of the linked `entries` into `<modules_dir>/.bin`. Each
/// entry's `target` is its link's destination, so no link is resolved.
pub(super) fn link_entry_bins(
    modules_dir: &Path,
    entries: &[ResolvedEntry<'_>],
    symlink: bool,
    bin_lookup: &crate::PrefetchedBinLookup<'_>,
    link_options: &LinkBinsOptions,
) -> Result<HashSet<String>, LinkBinsError> {
    let pass = crate::BinsDirPass::new(link_options);
    let linked = if symlink {
        let deps = resolved_entry_bins(entries);
        crate::link_bins_dir(modules_dir, crate::BinCandidates::Prefetched(&deps, bin_lookup), pass)
    } else {
        let locations: Vec<PathBuf> = entries
            .iter()
            .map(|entry| entry.target.clone())
            .collect();
        crate::link_bins_dir(modules_dir, crate::BinCandidates::Locations(&locations), pass)
    };
    linked.map(|linked| linked.linked_commands)
}

fn resolved_entry_bins(entries: &[ResolvedEntry<'_>]) -> Vec<crate::PrefetchedDepBin> {
    entries
        .iter()
        .map(|entry| {
            let snapshot_key = entry.spec.version.resolved_key(entry.name);
            (entry.name_str.clone(), entry.target.clone(), snapshot_key)
        })
        .collect()
}
