use miette::IntoDiagnostic;
use pnpm_config::{Config, NodeLinker};
use pnpm_deps_restorer::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;
use pnpm_modules_yaml::{Host, read_modules_manifest};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

/// Where each listed package is installed: its virtual store slot, or
/// the directory the hoisted linker placed it in.
pub(super) struct PackageDirs {
    layout: VirtualStoreLayout,
    /// The directories a `nodeLinker: hoisted` install, which leaves the
    /// virtual store empty, recorded in `.modules.yaml`, keyed by
    /// dependency path.
    hoisted_dirs: BTreeMap<String, PathBuf>,
}

impl PackageDirs {
    pub(super) fn new(
        config: &Config,
        lockfile_dir: &Path,
        layout: VirtualStoreLayout,
    ) -> miette::Result<Self> {
        if config.node_linker != NodeLinker::Hoisted {
            return Ok(Self { layout, hoisted_dirs: BTreeMap::new() });
        }
        let hoisted_locations =
            read_modules_manifest::<Host>(&lockfile_dir.join(config.modules_dir_name()))
                .into_diagnostic()?
                .and_then(|modules| modules.hoisted_locations)
                .unwrap_or_default();
        let mut hoisted_dirs = BTreeMap::new();
        for (dep_path, locations) in hoisted_locations {
            let Some(dir) =
                locations.first().and_then(|location| hoisted_dir(lockfile_dir, location))
            else {
                continue;
            };
            // The hoisted linker collapses the peer variants of one
            // package version onto the first dependency path it meets,
            // so only that one is recorded.
            if let Ok(key) = dep_path.parse::<PackageKey>() {
                hoisted_dirs
                    .entry(key.without_peer().to_string())
                    .or_insert_with(|| dir.clone());
            }
            hoisted_dirs.insert(dep_path, dir);
        }
        Ok(Self { layout, hoisted_dirs })
    }

    pub(super) fn package_dir(&self, key: &PackageKey, name: &str) -> PathBuf {
        let hoisted_dir = self.hoisted_dirs
            .get(&key.to_string())
            .or_else(|| self.hoisted_dirs.get(&key.without_peer().to_string()));
        match hoisted_dir {
            Some(dir) => dir.clone(),
            None => self.layout
                .slot_dir(key)
                .join("node_modules")
                .join(name),
        }
    }
}

/// A lockfile-relative location resolved against `lockfile_dir`, rebuilt
/// component by component so a location recorded with `/` gets the
/// platform's separator. `None` for a location that could leave
/// `lockfile_dir`.
fn hoisted_dir(lockfile_dir: &Path, location: &str) -> Option<PathBuf> {
    let mut dir = lockfile_dir.to_path_buf();
    for component in Path::new(location).components() {
        match component {
            Component::Normal(part) => dir.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(dir)
}

#[cfg(test)]
mod tests;
