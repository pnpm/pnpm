//! The materialization's inputs, built from the parts of the run.

use super::{
    super::{
        Arc, AtomicU8, IncludedDependencies, PackageManifest, PathBuf, RebuildOptions,
        prior_hoisted_dependencies, prior_hoisted_locations,
    },
    Dispatched, InstallScope, Loaded, Lockfiles, Verification,
};

/// The install's borrowed and `Copy` inputs, as one value every phase reads.
pub(super) fn materialization_lockfiles<'r, 'install>(
    loaded: &'r mut Loaded<'_>,
    lockfiles: &'r Lockfiles<'_>,
    dispatched: &mut Dispatched<'install>,
    verification: Verification,
) -> super::super::materialize::MaterializationLockfiles<'r, 'install> {
    super::super::materialize::MaterializationLockfiles {
        wanted: lockfiles.wanted.get(),
        wanted_shared: lockfiles.wanted.loader_handle(loaded.wanted.shared.take()),
        merge_wanted: loaded.wanted.merge,
        current: loaded.current.as_ref(),
        verification,
        verification_override: dispatched.modules.lockfile_verification_override.take(),
    }
}

impl super::InstallWorkspace<'_> {
    pub(super) fn materialization_workspace<'r>(
        &'r self,
        (dependency_groups, lockfile_specifier_project_manifests): (
            Vec<pnpm_package_manifest::DependencyGroup>,
            Option<Vec<(PathBuf, PackageManifest)>>,
        ),
        (project_manifests, workspace_projects): (
            &'r [(PathBuf, &'r PackageManifest)],
            Option<&'r [pnpm_workspace::Project]>,
        ),
        scope: &'r InstallScope<'_>,
    ) -> crate::install::materialize::MaterializationWorkspace<'r> {
        crate::install::materialize::MaterializationWorkspace {
            dependency_groups,
            project_manifests,
            lockfile_specifier_project_manifests,
            workspace_projects,
            requested_importer_ids: scope.importers.requested_importer_ids.as_ref(),
            real_importer_ids: &scope.importers.real_importer_ids,
            workspace_root: &self.dirs.workspace_root,
            catalogs: &self.catalogs,
        }
    }
}

impl Dispatched<'_> {
    pub(super) fn materialization_modules<'r>(
        &'r self,
        (included, rebuild): (IncludedDependencies, Option<&'r RebuildOptions>),
        prune_orphans: bool,
        logged_methods: &'r AtomicU8,
    ) -> crate::install::materialize::MaterializationModules<'r> {
        let prior_modules = self.modules.previous_modules_metadata.as_ref();
        crate::install::materialize::MaterializationModules {
            included,
            rebuild,
            modules_manifest: self.modules.old_modules.as_ref(),
            prior_hoisted_dependencies: prior_hoisted_dependencies(prior_modules),
            prior_hoisted_locations: prior_hoisted_locations(prior_modules),
            prune_orphans,
            relink_every_slot_bin: self.modules.tree_moved,
            logged_methods,
        }
    }
}

impl super::RunMode {
    pub(super) fn materialization_execution<'r>(
        &mut self,
        (owned, execution): (&'r super::InstallOwned, super::InstallExecution),
        options: &super::InstallRunOptions<'_, '_>,
        take_frozen_path: bool,
        early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
        prefix: &'r str,
    ) -> crate::install::materialize::MaterializationExecution<'r> {
        crate::install::materialize::MaterializationExecution {
            effective_node_version: self.effective_node_version.take(),
            take_frozen_path,
            supported_architectures: owned.projects.supported_architectures.as_ref(),
            early_host_detection,
            resolve_only: self.resolve_only(execution),
            can_prompt: self.can_prompt,
            save_lockfile: options.save.lockfile,
            prefix,
        }
    }
}

impl From<&super::InstallOwned> for crate::install::materialize::MaterializationDownloads {
    fn from(owned: &super::InstallOwned) -> Self {
        Self {
            tarball_mem_cache: Arc::clone(&owned.tarball_mem_cache),
            prefetch_downloads: None,
            http_client_arc: Arc::clone(&owned.http_client_arc),
            fetch_caches: owned.shared_caches().map(|caches| caches.fetch.clone()),
        }
    }
}

impl Lockfiles<'_> {
    pub(super) fn write_policy(
        &self,
        save: bool,
    ) -> crate::install::state_options::LockfileWritePolicy {
        crate::install::state_options::LockfileWritePolicy {
            synthesized_from_current: self.wanted.synthesized_from_current(),
            fast_updated: self.wanted.was_fast_updated(),
            save,
        }
    }
}
