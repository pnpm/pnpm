use super::{
    Command, ExcludedBins, FsWalkFiles, PackageBinSource, get_bins_from_package_manifest,
    pick_winner,
};
use std::collections::{HashMap, HashSet};

/// The bins `packages` provide, minus `exclude_bins`, each paired with the
/// package providing it. A name several packages provide goes to the one
/// that owns it, else to the first by name and highest version.
#[must_use]
pub fn choose_bins<'packages, Sys: FsWalkFiles>(
    packages: &'packages [PackageBinSource],
    exclude_bins: &std::collections::HashSet<String>,
) -> Vec<(Command, &'packages PackageBinSource)> {
    choose_bins_with(packages, exclude_bins, |pkg| {
        get_bins_from_package_manifest::<Sys>(&pkg.manifest, &pkg.location)
    })
}

pub(super) fn choose_bins_with<'packages>(
    packages: &'packages [PackageBinSource],
    exclude_bins: &HashSet<String>,
    commands: impl Fn(&PackageBinSource) -> Vec<Command>,
) -> Vec<(Command, &'packages PackageBinSource)> {
    let mut chosen: HashMap<String, (Command, &PackageBinSource)> = HashMap::new();
    for pkg in packages {
        for command in commands(pkg) {
            let wins = chosen
                .get(&command.name)
                .is_none_or(|(_, existing)| pick_winner(&command.name, existing, pkg));
            if wins {
                chosen.insert(command.name.clone(), (command, pkg));
            }
        }
    }
    let excluded = ExcludedBins::new(exclude_bins);
    chosen.retain(|name, _| !excluded.contains(name));
    chosen.into_values().collect()
}
