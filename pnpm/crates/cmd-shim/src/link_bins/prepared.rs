use super::{
    Command, FsCreateDirAll, FsEnsureExecutableBits, FsReadHead, FsReadToString, FsSetExecutable,
    FsWalkFiles, FsWrite, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimTargetCache,
    choose_bins_with, get_bins_from_package_manifest, link_chosen_bins,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(test)]
mod tests;

/// A manifest and its parsed bin declarations from the same package snapshot.
#[derive(Clone)]
pub struct PreparedPackageBins {
    pub(super) source: PackageBinSource,
    pub(super) commands: Arc<[Command]>,
}

impl PreparedPackageBins {
    #[must_use]
    pub fn source(&self) -> &PackageBinSource {
        &self.source
    }

    #[must_use]
    pub fn with_build_pending(mut self, pending: bool) -> Self {
        self.source.build_pending = pending;
        self
    }

    #[must_use]
    pub fn new<Sys: FsWalkFiles>(source: PackageBinSource) -> Self {
        let commands = get_bins_from_package_manifest::<Sys>(&source.manifest, &source.location);
        Self { source, commands: commands.into() }
    }
}

/// Reuse commands only for matching manifest snapshots; preserve full conflict selection and
/// target probes, executable permissions, and shim validation for every final winner.
pub fn link_bins_of_packages_precomputed<Sys>(
    packages: &[PackageBinSource],
    prepared: &HashMap<PathBuf, PreparedPackageBins>,
    bins_dir: &Path,
    options: &LinkBinsOptions,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    let chosen = choose_bins_with(packages, &HashSet::new(), |package| {
        prepared
            .get(&package.location)
            .filter(|cached| {
                cached.source.location == package.location
                    && Arc::ptr_eq(&cached.source.manifest, &package.manifest)
            })
            .map_or_else(
                || get_bins_from_package_manifest::<Sys>(&package.manifest, &package.location),
                |cached| cached.commands.to_vec(),
            )
    });
    link_chosen_bins::<Sys>(chosen, bins_dir, options, &ShimTargetCache::default()).map(|_| ())
}
