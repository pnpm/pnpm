//! The projects the on-disk phases link.

use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

pub(in crate::install_with_fresh_lockfile) struct OnDiskProjects<'a> {
    pub(in crate::install_with_fresh_lockfile) materialization_lockfile: &'a Lockfile,
    /// The whole wanted lockfile, with every variant's own snapshot.
    pub(in crate::install_with_fresh_lockfile) platform_lockfile:
        pnpm_deps_restorer::PlatformLockfile<'a>,
    pub(in crate::install_with_fresh_lockfile) importer_manifests:
        &'a BTreeMap<String, &'a PackageManifest>,
    pub(in crate::install_with_fresh_lockfile) project_anchor_importer_ids: &'a HashSet<String>,
}

impl OnDiskProjects<'_> {
    /// The manifests of the importers the install anchors on.
    pub(super) fn anchor_manifests(
        &self,
        workspace_root: &Path,
    ) -> Vec<(PathBuf, &PackageManifest)> {
        self.manifests(workspace_root, |id| self.project_anchor_importer_ids.contains(id))
    }

    /// The manifests of the importers in `on_disk`, or of every importer when
    /// it is `None`. A filtered install also resolves the importers its
    /// previous lockfile records for projects missing from disk, and links
    /// nothing into them.
    pub(super) fn on_disk_manifests(
        &self,
        workspace_root: &Path,
        on_disk: Option<&HashSet<String>>,
    ) -> Vec<(PathBuf, &PackageManifest)> {
        self.manifests(workspace_root, |id| on_disk.is_none_or(|on_disk| on_disk.contains(id)))
    }

    fn manifests(
        &self,
        workspace_root: &Path,
        includes: impl Fn(&str) -> bool,
    ) -> Vec<(PathBuf, &PackageManifest)> {
        self.importer_manifests
            .iter()
            .filter(|(id, _)| includes(id))
            .map(|(id, manifest)| (workspace_root.join(id), *manifest))
            .collect()
    }

    pub(super) fn root_component_importers(&self) -> HashSet<String> {
        self.importer_manifests
            .iter()
            .filter(|(id, _)| self.project_anchor_importer_ids.contains(id.as_str()))
            .filter(|(_, manifest)| {
                manifest.install_config_hoisting_limits()
                    == Some(pnpm_deps_restorer::HOISTING_LIMITS_WORKSPACES)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }
}
