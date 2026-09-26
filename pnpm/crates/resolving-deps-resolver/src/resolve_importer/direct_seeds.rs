use crate::resolve_dependency_tree::{WantedSpec, importer_direct_wanted_specs};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use rustc_hash::FxHashSet as HashSet;
use std::collections::BTreeMap;

use super::{ResolveImporterError, ResolveImporterOptions};

pub(crate) struct DirectSeeds {
    pub(crate) initial_wanted: Vec<WantedSpec>,
    pub(crate) wanted_specifier_by_alias: BTreeMap<String, String>,
    pub(crate) parent_pkg_aliases: HashSet<String>,
}

impl DirectSeeds {
    pub(crate) fn of<DependencyGroupList>(
        manifest: &PackageManifest,
        dependency_groups: DependencyGroupList,
        opts: &ResolveImporterOptions,
    ) -> Result<Self, ResolveImporterError>
    where
        DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    {
        let initial_wanted = importer_direct_wanted_specs(
            manifest,
            dependency_groups,
            opts.peers.auto_install_peers,
            &opts.resolution.catalogs,
            opts.resolution.catalogs_dir.as_deref(),
        )?;
        Ok(Self {
            wanted_specifier_by_alias: initial_wanted
                .iter()
                .map(|(alias, range, ..)| (alias.clone(), range.clone()))
                .collect(),
            parent_pkg_aliases: initial_wanted
                .iter()
                .map(|(alias, ..)| alias.clone())
                .collect(),
            initial_wanted,
        })
    }
}
