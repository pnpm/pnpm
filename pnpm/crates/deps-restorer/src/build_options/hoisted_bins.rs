use pnpm_cmd_shim::{DirectoryBinPlan, Host, LinkBinsError, LinkBinsOptions, PackageBinSource};
use pnpm_lockfile::PackageKey;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(crate) struct HoistedBinPlans {
    directories: Mutex<HashMap<PathBuf, Arc<Mutex<HoistedBinDirectory>>>>,
    private_hoist: Mutex<Option<PrivateHoistedBins>>,
}

struct PrivateHoistedBins {
    directory: PathBuf,
    locations_by_key: HashMap<PackageKey, Vec<PathBuf>>,
}

struct MutatedPrivateHoist {
    directory: PathBuf,
    completed: HashSet<PackageKey>,
    locations_by_key: HashMap<PackageKey, Vec<PathBuf>>,
}

#[derive(Default)]
pub(crate) struct HoistedBinDirectory {
    pub(crate) plan: Option<DirectoryBinPlan>,
    pub(crate) completed: HashSet<PackageKey>,
    sources: Option<Arc<[PackageBinSource]>>,
}

impl HoistedBinDirectory {
    pub(crate) fn initialize(&mut self, directory: &Path) -> Result<(), LinkBinsError> {
        if self.plan.is_none() {
            self.plan = Some(match self.sources.take() {
                Some(sources) => DirectoryBinPlan::from_packages::<Host>(&sources),
                None => DirectoryBinPlan::discover::<Host>(directory)?,
            });
        }
        Ok(())
    }

    pub(crate) fn plan_or_seeded(&mut self) -> Option<&DirectoryBinPlan> {
        if self.plan.is_none() {
            let sources = self.sources.take()?;
            self.plan = Some(DirectoryBinPlan::from_packages::<Host>(&sources));
        }
        self.plan.as_ref()
    }
}

impl HoistedBinPlans {
    pub(crate) fn seed(&self, sources: &crate::HoistedBinSources) {
        let mut directories =
            self.directories.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for (directory, sources) in sources {
            directories.insert(
                directory.clone(),
                Arc::new(Mutex::new(HoistedBinDirectory {
                    sources: Some(Arc::clone(sources)),
                    ..HoistedBinDirectory::default()
                })),
            );
        }
    }

    pub(crate) fn seed_private_hoist(&self, directory: PathBuf, aliases: &[(String, PackageKey)]) {
        if aliases.is_empty() {
            return;
        }
        let mut locations_by_key: HashMap<PackageKey, Vec<PathBuf>> = HashMap::new();
        for (alias, key) in aliases {
            locations_by_key
                .entry(key.clone())
                .or_default()
                .push(pnpm_fs::join_slash_separated_path(&directory, alias));
        }
        *self.private_hoist.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(PrivateHoistedBins { directory, locations_by_key });
    }

    pub(crate) fn initialize_private_hoist_for(
        &self,
        key: &PackageKey,
    ) -> Result<(), LinkBinsError> {
        let directory = {
            let guard =
                self.private_hoist.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(private_hoist) = guard.as_ref() else { return Ok(()) };
            if !private_hoist.locations_by_key.contains_key(key) {
                return Ok(());
            }
            private_hoist.directory.clone()
        };
        let entry = self.directory(&directory);
        let mut entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        entry.initialize(&directory)
    }

    pub(crate) fn refresh_private_hoist_for(
        &self,
        key: &PackageKey,
        base_options: &LinkBinsOptions,
    ) -> Result<(), LinkBinsError> {
        let (directory, locations) = {
            let guard =
                self.private_hoist.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(private_hoist) = guard.as_ref() else { return Ok(()) };
            let Some(locations) = private_hoist.locations_by_key.get(key) else {
                return Ok(());
            };
            (
                private_hoist.directory.clone(),
                locations
                    .iter()
                    .cloned()
                    .collect::<HashSet<PathBuf>>(),
            )
        };
        let entry = self.directory(&directory);
        let mut entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if entry.completed.contains(key) {
            return Ok(());
        }
        entry.initialize(&directory)?;
        let mut options = base_options.clone();
        options.force = true;
        let plan = entry.plan.as_mut().expect("private hoist bin plan initialized");
        plan.refresh::<Host>(&locations, &directory.join(".bin"), &options)?;
        entry.completed.insert(key.clone());
        Ok(())
    }

    pub(crate) fn refresh_private_hoist(
        &self,
        mutated: &HashSet<PackageKey>,
        base_options: &LinkBinsOptions,
    ) -> Result<(), LinkBinsError> {
        let Some(MutatedPrivateHoist {
            directory,
            completed,
            locations_by_key,
        }) = self.collect_mutated_private_hoist(mutated)
        else {
            return Ok(());
        };
        let entry = self.directory(&directory);
        let mut entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if completed.is_subset(&entry.completed) {
            return Ok(());
        }
        entry.initialize(&directory)?;
        let pending: HashSet<PackageKey> = completed
            .difference(&entry.completed)
            .cloned()
            .collect();
        let locations: HashSet<PathBuf> = pending
            .iter()
            .filter_map(|key| locations_by_key.get(key))
            .flatten()
            .cloned()
            .collect();
        let mut options = base_options.clone();
        options.force = true;
        let plan = entry.plan.as_mut().expect("private hoist bin plan initialized");
        plan.refresh::<Host>(&locations, &directory.join(".bin"), &options)?;
        entry.completed.extend(completed);
        Ok(())
    }

    fn collect_mutated_private_hoist(
        &self,
        mutated: &HashSet<PackageKey>,
    ) -> Option<MutatedPrivateHoist> {
        if mutated.is_empty() {
            return None;
        }
        let guard = self.private_hoist.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let private_hoist = guard.as_ref()?;
        let mut completed = HashSet::new();
        let mut locations_by_key: HashMap<PackageKey, Vec<PathBuf>> = HashMap::new();
        for key in mutated {
            if let Some(locations) = private_hoist.locations_by_key.get(key) {
                completed.insert(key.clone());
                locations_by_key.insert(key.clone(), locations.clone());
            }
        }
        (!completed.is_empty()).then(|| MutatedPrivateHoist {
            directory: private_hoist.directory.clone(),
            completed,
            locations_by_key,
        })
    }

    pub(crate) fn existing_directory(
        &self,
        path: &Path,
    ) -> Option<Arc<Mutex<HoistedBinDirectory>>> {
        self.directories
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(path)
            .map(Arc::clone)
    }

    pub(crate) fn directory(&self, path: &Path) -> Arc<Mutex<HoistedBinDirectory>> {
        Arc::clone(
            self.directories
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(path.to_owned())
                .or_default(),
        )
    }
}
