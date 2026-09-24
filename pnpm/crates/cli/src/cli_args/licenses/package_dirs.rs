use miette::IntoDiagnostic;
use pnpm_config::{Config, NodeLinker};
use pnpm_deps_restorer::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;
use pnpm_modules_yaml::{Host, read_modules_manifest};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Where each listed package is installed: its virtual store slot, or
/// the directory the hoisted linker placed it in.
pub(super) struct PackageDirs {
    layout: VirtualStoreLayout,
    lockfile_dir: PathBuf,
    /// Lockfile-relative directories keyed by dependency path, recorded
    /// in `.modules.yaml` by a `nodeLinker: hoisted` install, which
    /// leaves the virtual store empty.
    hoisted_locations: BTreeMap<String, Vec<String>>,
}

impl PackageDirs {
    pub(super) fn new(
        config: &Config,
        lockfile_dir: &Path,
        layout: VirtualStoreLayout,
    ) -> miette::Result<Self> {
        let hoisted_locations = if config.node_linker == NodeLinker::Hoisted {
            read_modules_manifest::<Host>(&lockfile_dir.join(config.modules_dir_name()))
                .into_diagnostic()?
                .and_then(|modules| modules.hoisted_locations)
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        Ok(Self { layout, lockfile_dir: lockfile_dir.to_path_buf(), hoisted_locations })
    }

    pub(super) fn package_dir(&self, key: &PackageKey, name: &str) -> PathBuf {
        let hoisted_location = self.hoisted_locations
            .get(&key.to_string())
            .and_then(|locations| locations.first());
        match hoisted_location {
            // Rebuilt component by component so a location recorded
            // with `/` separators gets the platform's separator.
            Some(location) => {
                let mut package_dir = self.lockfile_dir.clone();
                package_dir.extend(Path::new(location).components());
                package_dir
            }
            None => self.layout
                .slot_dir(key)
                .join("node_modules")
                .join(name),
        }
    }
}
