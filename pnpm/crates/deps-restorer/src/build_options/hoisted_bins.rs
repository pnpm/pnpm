use pnpm_cmd_shim::{DirectoryBinPlan, Host, LinkBinsError, PackageBinSource};
use pnpm_lockfile::PackageKey;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(crate) struct HoistedBinPlans {
    directories: Mutex<HashMap<PathBuf, Arc<Mutex<HoistedBinDirectory>>>>,
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
            self.plan = Some(match &self.sources {
                Some(sources) => DirectoryBinPlan::from_packages::<Host>(sources),
                None => DirectoryBinPlan::discover::<Host>(directory)?,
            });
            self.sources = None;
        }
        Ok(())
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
