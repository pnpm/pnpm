use super::{
    Command, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead,
    FsReadToString, FsSetExecutable, FsWalkFiles, FsWrite, LinkBinsError, LinkBinsOptions,
    PackageBinSource, ShimTargetCache, collect_packages_in_modules_dir, discovery::read_package,
    get_bins_from_package_manifest, link_chosen_bins, pick_winner,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

mod removal;

#[cfg(test)]
mod tests;

/// Installed command candidates for one modules directory during a build phase.
/// Discover before package mutations; refresh completed providers before their consumers run.
/// Unchanged providers retain their parsed manifests and bin declarations.
#[derive(Default)]
pub struct DirectoryBinPlan {
    package_indices: HashMap<PathBuf, usize>,
    packages: Vec<Provider>,
    candidates: HashMap<String, Vec<(usize, usize)>>,
    pending: HashSet<String>,
}

#[derive(Default)]
struct Provider {
    source: Option<PackageBinSource>,
    commands: Arc<[Command]>,
}

impl DirectoryBinPlan {
    /// Clone a provider's latest manifest snapshot; absent locations require a disk lookup.
    #[must_use]
    pub fn package_source(&self, location: &Path) -> Option<PackageBinSource> {
        self.packages
            .get(*self.package_indices.get(location)?)?
            .source
            .clone()
    }

    /// Snapshot one provider's manifest and validated commands without re-parsing its bins.
    #[must_use]
    pub fn package_bins(&self, location: &Path) -> Option<super::PreparedPackageBins> {
        let provider = self.packages.get(*self.package_indices.get(location)?)?;
        Some(super::PreparedPackageBins {
            source: provider.source.clone()?,
            commands: Arc::clone(&provider.commands),
        })
    }

    pub fn discover<Sys>(directory: &Path) -> Result<Self, LinkBinsError>
    where
        Sys: FsReadDir + FsReadFile + FsWalkFiles,
    {
        Ok(Self::from_packages::<Sys>(&collect_packages_in_modules_dir::<Sys>(directory)?))
    }

    /// Seed candidates from the same ordered manifest snapshots used to link this directory.
    #[must_use]
    pub fn from_packages<Sys: FsWalkFiles>(packages: &[PackageBinSource]) -> Self {
        let mut plan = Self {
            package_indices: HashMap::with_capacity(packages.len()),
            packages: Vec::with_capacity(packages.len()),
            candidates: HashMap::with_capacity(packages.len()),
            ..Self::default()
        };
        for package in packages {
            plan.insert_package::<Sys>(package);
        }
        plan
    }

    /// Reread only completed providers, retaining conflict resolution against all candidates.
    /// Failed writes stay pending for a retry; removed commands lose their platform shims.
    pub fn refresh<Sys>(
        &mut self,
        completed: &HashSet<PathBuf>,
        bins_dir: &Path,
        options: &LinkBinsOptions,
    ) -> Result<(), LinkBinsError>
    where
        Sys: FsReadFile
            + FsReadToString
            + FsReadHead
            + FsCreateDirAll
            + FsWalkFiles
            + FsWrite
            + FsSetExecutable
            + FsEnsureExecutableBits,
    {
        for location in completed {
            self.update_package::<Sys>(location)?;
        }
        let mut chosen = Vec::new();
        let mut provided = None;
        for name in &self.pending {
            if let Some((command, package)) = self.winner(name) {
                chosen.push((command.clone(), package));
            } else {
                let provided = provided.get_or_insert_with(|| self.provided_names());
                removal::remove_unclaimed(name, bins_dir, provided)?;
            }
        }
        link_chosen_bins::<Sys>(chosen, bins_dir, options, &ShimTargetCache::default())?;
        self.pending.clear();
        Ok(())
    }

    fn provided_names(&self) -> HashSet<String> {
        self.candidates
            .iter()
            .filter(|(_, candidates)| !candidates.is_empty())
            .map(|(name, _)| removal::normalize(name, cfg!(windows)))
            .collect()
    }

    fn update_package<Sys: FsReadFile + FsWalkFiles>(
        &mut self,
        location: &Path,
    ) -> Result<(), LinkBinsError> {
        let package = read_package::<Sys>(location)?;
        let old_names = self.package_indices
            .get(location)
            .map(|index| {
                self.packages[*index].commands
                    .iter()
                    .map(|command| command.name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let new_names: Vec<_> = package
            .as_ref()
            .map(|package| get_bins_from_package_manifest::<Sys>(&package.manifest, location))
            .unwrap_or_default();
        let affected: HashSet<_> = old_names
            .iter()
            .cloned()
            .chain(new_names.iter().map(|command| command.name.clone()))
            .collect();
        let before: Vec<_> = affected
            .into_iter()
            .map(|name| {
                let winner = self.winner_candidate(&name).map(|(_, index)| *index);
                (name, winner)
            })
            .collect();
        self.remove_package(location, &old_names);
        if let Some(package) = package {
            self.insert_commands(&package, new_names);
        }
        let changed = self.package_indices.get(location).copied();
        for (name, previous) in before {
            let winner = self.winner_candidate(&name).map(|(_, index)| *index);
            if winner.is_some() && winner == changed || winner != previous {
                self.pending.insert(name);
            }
        }
        Ok(())
    }

    fn insert_package<Sys: FsWalkFiles>(&mut self, package: &PackageBinSource) {
        let commands = get_bins_from_package_manifest::<Sys>(&package.manifest, &package.location);
        self.insert_commands(package, commands);
    }

    fn insert_commands(&mut self, package: &PackageBinSource, commands: Vec<Command>) {
        let index = *self.package_indices
            .entry(package.location.clone())
            .or_insert_with(|| {
                let index = self.packages.len();
                self.packages.push(Provider::default());
                index
            });
        for (command_index, command) in commands.iter().enumerate() {
            let candidates = self.candidates.entry(command.name.clone()).or_default();
            let position = candidates.partition_point(|(_, candidate)| *candidate <= index);
            candidates.insert(position, (command_index, index));
        }
        self.packages[index] =
            Provider { source: Some(package.clone()), commands: commands.into() };
    }

    fn remove_package(&mut self, location: &Path, names: &[String]) {
        let Some(&index) = self.package_indices.get(location) else { return };
        self.packages[index] = Provider::default();
        for name in names {
            if let Some(candidates) = self.candidates.get_mut(name) {
                candidates.retain(|(_, candidate)| *candidate != index);
            }
        }
    }

    fn winner(&self, name: &str) -> Option<(&Command, &PackageBinSource)> {
        self.winner_candidate(name)
            .map(|(command, index)| {
                (&self.packages[*index].commands[*command], self.provider_source(*index))
            })
    }

    fn winner_candidate(&self, name: &str) -> Option<&(usize, usize)> {
        self.candidates
            .get(name)?
            .iter()
            .reduce(|existing, candidate| {
                if pick_winner(
                    name,
                    self.provider_source(existing.1),
                    self.provider_source(candidate.1),
                ) {
                    candidate
                } else {
                    existing
                }
            })
    }

    fn provider_source(&self, index: usize) -> &PackageBinSource {
        self.packages[index].source.as_ref().expect("active bin provider")
    }
}
