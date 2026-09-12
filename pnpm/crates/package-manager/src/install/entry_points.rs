use super::{
    Install, InstallRunOptions, ProjectMutation, WorkspaceInstallSelection, errors::InstallError,
};
use crate::{
    LockfileVerificationOverride, PolicyExcludes, RebuildOptions, ResolvedPackages,
    UpdateSeedPolicy,
};
use pnpm_config::Config;
use pnpm_lockfile::MaybeLazyLockfile;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_tarball::MemCache;
use std::{collections::HashSet, path::PathBuf, sync::Arc};

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Create a full install using the resolved configuration and no per-run overrides.
    /// Policy exclusions are not persisted unless the caller explicitly enables it.
    pub fn new(
        tarball_mem_cache: Arc<MemCache>,
        resolved_packages: &'a ResolvedPackages,
        http_client: (&'a ThrottledClient, Arc<ThrottledClient>),
        config: &'static Config,
        manifest: &'a PackageManifest,
        lockfile: MaybeLazyLockfile<'a>,
        dependency_groups: DependencyGroupList,
    ) -> Self {
        Self {
            tarball_mem_cache,
            resolved_packages,
            http_client: http_client.0,
            http_client_arc: http_client.1,
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile,
            lockfile_path: None,
            dependency_groups,
            frozen_lockfile: false,
            prefer_frozen_lockfile: None,
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: false,
            dry_run: false,
            policy_excludes: PolicyExcludes::Skip,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
    }

    /// Execute the subroutine.
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions::default())).await
    }

    /// Execute as a check: the caller compares the lockfile the run
    /// produced against the one it snapshotted and restores that snapshot,
    /// so nothing else on disk may be left changed. pnpm's
    /// `lockfileCheck`.
    pub async fn run_lockfile_check<Reporter: self::Reporter + 'static>(
        self,
        selection: Option<WorkspaceInstallSelection<'_>>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            lockfile_check: true,
            selection,
            ..Default::default()
        }))
        .await
    }

    pub(crate) async fn run_with_lockfile_specifier_project_manifests<
        Reporter: self::Reporter + 'static,
    >(
        self,
        lockfile_specifier_project_manifests: Vec<(PathBuf, PackageManifest)>,
        read_package_hooked_manifest_paths: HashSet<PathBuf>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            lockfile_specifier_project_manifests: Some(lockfile_specifier_project_manifests),
            read_package_hooked_manifest_paths,
            ..Default::default()
        }))
        .await
    }

    #[cfg(test)]
    pub(crate) async fn run_with_prompt_eligibility<Reporter: self::Reporter + 'static>(
        self,
        can_prompt: bool,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            prompt_eligibility_override: Some(can_prompt),
            ..Default::default()
        }))
        .await
    }

    pub async fn run_with_lockfile_verification<Reporter: self::Reporter + 'static>(
        self,
        lockfile_verification_override: LockfileVerificationOverride<'a>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            lockfile_verification_override: Some(lockfile_verification_override),
            ..Default::default()
        }))
        .await
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        selection: WorkspaceInstallSelection<'_>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            selection: Some(selection),
            ..Default::default()
        }))
        .await
    }

    pub(crate) async fn run_selected_with_lockfile_specifier_project_manifests<
        Reporter: self::Reporter + 'static,
    >(
        self,
        selection: WorkspaceInstallSelection<'_>,
        lockfile_specifier_project_manifests: Vec<(PathBuf, PackageManifest)>,
        read_package_hooked_manifest_paths: HashSet<PathBuf>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            selection: Some(selection),
            lockfile_specifier_project_manifests: Some(lockfile_specifier_project_manifests),
            read_package_hooked_manifest_paths,
            ..Default::default()
        }))
        .await
    }

    /// `pacquet update`'s install: the same run as [`Self::run`], with the
    /// declared ranges of `bumps`'s targets moved onto the versions the
    /// resolve settles on. See [`crate::ManifestSpecBumps`].
    pub async fn run_with_manifest_spec_bumps<Reporter: self::Reporter + 'static>(
        self,
        bumps: &'a crate::ManifestSpecBumps,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            manifest_spec_bumps: Some(bumps),
            ..Default::default()
        }))
        .await
    }

    /// [`Self::run_selected`] with the range rewrites of
    /// [`Self::run_with_manifest_spec_bumps`].
    pub async fn run_selected_with_manifest_spec_bumps<Reporter: self::Reporter + 'static>(
        self,
        selection: WorkspaceInstallSelection<'_>,
        bumps: &'a crate::ManifestSpecBumps,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            selection: Some(selection),
            manifest_spec_bumps: Some(bumps),
            ..Default::default()
        }))
        .await
    }

    pub async fn run_selected_with_lockfile_verification<Reporter: self::Reporter + 'static>(
        self,
        selection: WorkspaceInstallSelection<'_>,
        lockfile_verification_override: LockfileVerificationOverride<'a>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            lockfile_verification_override: Some(lockfile_verification_override),
            selection: Some(selection),
            ..Default::default()
        }))
        .await
    }

    /// Execute the install a legacy `pacquet deploy` runs in its target
    /// directory: the deployed manifest is the sole root importer, while
    /// workspace discovery stays anchored at the source workspace so
    /// `workspace:` dependencies still resolve to their projects.
    ///
    /// The source workspace also still owns `pnpm-lock.yaml`, and this
    /// resolution describes the deployed project rather than the
    /// workspace, so nothing is written to it (pnpm's
    /// `saveLockfile: false`).
    pub async fn run_legacy_deploy<Reporter: self::Reporter + 'static>(
        self,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            root_manifest_as_workspace_root: true,
            deploy_manifest_hook: true,
            save_lockfile: false,
            ..Default::default()
        }))
        .await
    }

    /// Execute as a forced rebuild: take the frozen path against the
    /// already-resolved lockfile + materialized `node_modules`, bypass the
    /// "up to date" short-circuit, and re-run the lifecycle scripts of the
    /// selected packages (or every build-needing package when
    /// `rebuild.selected_names` is `None`). Drives `pacquet rebuild` and
    /// the rebuild step of `pacquet approve-builds`.
    ///
    /// # Panics
    ///
    /// Panics unless `frozen_lockfile` is set: a rebuild must take the
    /// frozen path, since the fresh-resolve path drops the rebuild
    /// selection and would silently degrade to a plain install.
    pub async fn run_rebuild<Reporter: self::Reporter + 'static>(
        self,
        rebuild: RebuildOptions,
    ) -> Result<(), InstallError> {
        assert!(self.frozen_lockfile, "run_rebuild requires frozen_lockfile = true");
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            rebuild: Some(rebuild),
            ..Default::default()
        }))
        .await
    }

    /// Execute a forced rebuild limited to the selected workspace importers.
    pub async fn run_selected_rebuild<Reporter: self::Reporter + 'static>(
        self,
        selection: WorkspaceInstallSelection<'_>,
        rebuild: RebuildOptions,
    ) -> Result<(), InstallError> {
        assert!(self.frozen_lockfile, "run_selected_rebuild requires frozen_lockfile = true");
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            rebuild: Some(rebuild),
            selection: Some(selection),
            ..Default::default()
        }))
        .await
    }
}
pub fn apply_deploy_manifest_hook(manifest: &mut serde_json::Value) {
    let names = deploy_workspace_dependency_names(manifest).map(str::to_owned).collect::<Vec<_>>();
    inject_deploy_dependencies_meta(manifest, names);
}
pub(crate) fn apply_deploy_manifest_hook_to_arc(
    mut manifest: Arc<serde_json::Value>,
) -> Arc<serde_json::Value> {
    let names = deploy_workspace_dependency_names(&manifest).map(str::to_owned).collect::<Vec<_>>();
    if names.is_empty() {
        return manifest;
    }
    inject_deploy_dependencies_meta(Arc::make_mut(&mut manifest), names);
    manifest
}
pub(super) fn deploy_workspace_dependency_names(
    manifest: &serde_json::Value,
) -> impl Iterator<Item = &str> {
    ["optionalDependencies", "dependencies", "devDependencies"]
        .into_iter()
        .filter_map(move |field| manifest.get(field)?.as_object())
        .flat_map(|dependencies| dependencies.iter())
        .filter_map(|(name, specifier)| {
            specifier
                .as_str()
                .is_some_and(|specifier| specifier.starts_with("workspace:"))
                .then_some(name.as_str())
        })
}
pub(super) fn inject_deploy_dependencies_meta(
    manifest: &mut serde_json::Value,
    names: Vec<String>,
) {
    if names.is_empty() {
        return;
    }
    let Some(object) = manifest.as_object_mut() else { return };
    let dependencies_meta = object
        .entry("dependenciesMeta")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let Some(meta_object) = dependencies_meta.as_object_mut() else { return };
    for name in names {
        let dependency_meta = meta_object.entry(name).or_insert(serde_json::Value::Null);
        match dependency_meta {
            serde_json::Value::Object(object) => {
                object.insert("injected".to_owned(), serde_json::Value::Bool(true));
            }
            value => *value = serde_json::json!({ "injected": true }),
        }
    }
}
