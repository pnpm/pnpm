use super::{
    FrozenScope, HashSet, InstallError, InstallFrozenLockfile, Lockfile, LockfileEntries,
    MaterializationInputs, MaterializationOutput, MaterializationWorkspace, Materialized, Reporter,
    allow_builds_changed_since, anchored_project_manifests, announce_headless_install,
    frozen_project_anchor_ids, initial_materialization_ids, map_frozen_lockfile_error,
    previously_skipped, prior_unbuilt_builds, settle_frozen_verification,
};
#[cfg(not(feature = "runtime-provisioning"))]
use pnpm_lockfile::{ImporterDepVersion, Prefix};
#[cfg(not(feature = "runtime-provisioning"))]
use pnpm_package_manifest::DependencyGroup;

impl<'a> MaterializationInputs<'a, '_> {
    fn groups(&self, lockfile: &Lockfile) -> crate::GroupSelection {
        crate::GroupSelection::classify(
            lockfile,
            self.modules.included,
            self.install.context.config.peer_edge_options(),
        )
        .linking_for(self.execution.supported_architectures)
    }

    fn frozen_lockfiles<'b>(
        &'b self,
        scope: &'b FrozenScope<'a>,
        lockfile: &'b Lockfile,
    ) -> pnpm_deps_restorer::FrozenLockfileInputs<'b> {
        pnpm_deps_restorer::FrozenLockfileInputs {
            wanted: scope.lockfile(),
            verified: lockfile,
            path: self.lockfiles.verification.derived_lockfile_path.as_deref(),
            current: self.lockfiles.current,
            current_entries: LockfileEntries::of_previous_install(self.lockfiles.current),
            resolution_verifiers: self.workspace.requested_importer_ids.map_or(
                self.lockfiles.verification.resolution_verifiers.as_slice(),
                |_| &[][..],
            ),
            planned_canonical_fetches: Some(&self.lockfiles.verification.planned_canonical_fetches),
        }
    }

    fn take_frozen_seed<'b>(
        &mut self,
        frozen_verification_override: Option<crate::LockfileVerificationOverride<'b>>,
    ) -> pnpm_deps_restorer::FrozenInstallSeed<'b> {
        pnpm_deps_restorer::FrozenInstallSeed {
            early_host_detection: self.execution.early_host_detection.take(),
            node_version: self.execution.effective_node_version.take(),
            skipped: self.modules.modules_manifest.map(|manifest| {
                manifest.skipped.clone()
            }),
            lockfile_verification_override: frozen_verification_override,
            known_requires_build: self.resolution.known_requires_build.take(),
        }
    }

    fn frozen_installer<'b>(
        &'b mut self,
        scope: &'b FrozenScope<'a>,
        lockfile: &'b Lockfile,
        frozen_verification_override: Option<crate::LockfileVerificationOverride<'b>>,
        prior_unbuilt_builds: &'b pnpm_deps_restorer::UnbuiltBuilds,
        previously_skipped: &'b pnpm_deps_restorer::SkippedSnapshots,
    ) -> InstallFrozenLockfile<'b> {
        let seed = self.take_frozen_seed(frozen_verification_override);
        InstallFrozenLockfile {
            drivers: pnpm_deps_restorer::FrozenInstallDrivers {
                config: self.install.context.config,
                http_client: self.install.context.http_client,
                pnpmfile_hook: self.resolution.pnpmfile_hook.as_ref(),
                tarball_mem_cache: Some(&self.downloads.tarball_mem_cache),
                fetch_caches: self.downloads.fetch_caches.as_ref(),
            },
            lockfiles: self.frozen_lockfiles(scope, lockfile),
            platform: pnpm_deps_restorer::FrozenPlatformOptions {
                supported_architectures: self.execution.supported_architectures,
                skip_runtimes: self.install.execution.skip_runtimes,
                node_linker: self.install.execution.node_linker,
            },
            prior: pnpm_deps_restorer::PriorMaterialization {
                rebuild: self.modules.rebuild,
                hoisted_dependencies: self.modules.prior_hoisted_dependencies,
                hoisted_locations: self.modules.prior_hoisted_locations,
                allow_builds_changed: allow_builds_changed_since(
                    self.modules.modules_manifest,
                    self.install.context.config,
                ),
                unbuilt_builds: prior_unbuilt_builds,
                previously_skipped,
                prune: self.modules.prune,
                relink_every_slot_bin: self.modules.relink_every_slot_bin,
            },
            projects: pnpm_deps_restorer::FrozenProjectInputs {
                workspace_root: self.workspace.workspace_root,
                requester: self.execution.prefix,
                dependency_groups: &self.workspace.dependency_groups,
                groups: &scope.installer_groups,
                manifests: &scope.project_manifests,
                package_map_manifests: self.workspace.project_manifests,
            },
            seed,

            logged_methods: self.modules.logged_methods,
        }
    }

    pub(super) async fn frozen<Reporter: self::Reporter + 'static>(
        mut self,
    ) -> Result<MaterializationOutput, InstallError> {
        let lockfile = self.lockfiles.wanted.expect("dispatch verified lockfile is present");
        announce_headless_install::<Reporter>(
            lockfile,
            self.modules.rebuild,
            self.install.lockfile_policy.ignore_manifest_check
                && !self.install.execution.mutation.is_full_install(),
            self.execution.prefix,
        );
        let scope = self.workspace.frozen_scope(
            lockfile,
            self.hoisted_prior(),
            self.groups(lockfile),
            self.install.lockfile_policy.ignore_manifest_check,
        );
        #[cfg(not(feature = "runtime-provisioning"))]
        ensure_runtime_provisioning(&scope, self.install.execution.skip_runtimes)?;
        assert_supported_lockfile_major(scope.lockfile());

        let frozen_verification_override = settle_frozen_verification::<Reporter>(
            self.workspace.requested_importer_ids,
            self.lockfiles.verification_override.take(),
            lockfile,
            &self.lockfiles.verification.resolution_verifiers,
            self.lockfiles.verification.derived_lockfile_path.as_deref(),
            &self.install.context.config.cache_dir,
        )
        .await?;
        let prior_unbuilt = prior_unbuilt_builds(self.modules.modules_manifest);
        let prior_skipped = previously_skipped(self.modules.modules_manifest);
        let frozen_result = self
            .frozen_installer(
                &scope,
                lockfile,
                frozen_verification_override,
                &prior_unbuilt,
                &prior_skipped,
            )
            .run::<Reporter>()
            .await
            // Surface a verification failure as the same top-level
            // `LockfileVerification` variant the eager paths use, rather
            // than nesting it under `FrozenLockfile` — the concurrent gate
            // is the same gate, run alongside the fetch.
            .map_err(map_frozen_lockfile_error)?;
        Ok(MaterializationOutput::from_frozen(frozen_result, scope.groups))
    }
}

fn assert_supported_lockfile_major(lockfile: &Lockfile) {
    let major = lockfile.lockfile_version.major;
    let supported = matches!(major, 9 | 12 | 13);
    debug_assert!(supported);
}

#[cfg(not(feature = "runtime-provisioning"))]
fn ensure_runtime_provisioning(
    scope: &FrozenScope<'_>,
    skip_runtimes: bool,
) -> Result<(), InstallError> {
    if skip_runtimes {
        return Ok(());
    }
    if let Some(alias) = selected_runtime_dependency(
        &scope.closure.lockfile,
        &scope.closure.importer_ids,
        scope.groups.included,
    ) {
        return Err(InstallError::RuntimeProvisioningUnavailable { alias });
    }
    Ok(())
}

#[cfg(not(feature = "runtime-provisioning"))]
fn selected_runtime_dependency(
    lockfile: &Lockfile,
    importer_ids: &HashSet<String>,
    included: pnpm_modules_yaml::IncludedDependencies,
) -> Option<String> {
    importer_ids
        .iter()
        .filter_map(|importer_id| lockfile.importers.get(importer_id))
        .flat_map(|importer| {
            importer.dependencies_by_groups(
                [
                    included.dependencies.then_some(DependencyGroup::Prod),
                    included.dev_dependencies.then_some(DependencyGroup::Dev),
                    included.optional_dependencies.then_some(DependencyGroup::Optional),
                ]
                .into_iter()
                .flatten(),
            )
        })
        .find_map(|(alias, dependency)| {
            let runtime = match &dependency.version {
                ImporterDepVersion::Regular(version) => version.prefix() == Prefix::Runtime,
                ImporterDepVersion::Alias(version) => version.suffix.prefix() == Prefix::Runtime,
                ImporterDepVersion::Link(_) | ImporterDepVersion::File(_) => false,
            };
            runtime.then(|| alias.to_string())
        })
        .or_else(|| {
            lockfile.snapshots
                .iter()
                .flat_map(|snapshots| snapshots.values())
                .flat_map(|snapshot| {
                    snapshot.dependencies
                        .iter()
                        .chain(
                            included.optional_dependencies
                                .then_some(snapshot.optional_dependencies.as_ref())
                                .flatten()
                                .iter(),
                        )
                })
                .flat_map(|dependencies| dependencies.iter())
                .find_map(|(alias, dependency)| {
                    dependency
                        .ver_peer()
                        .is_some_and(|version| version.prefix() == Prefix::Runtime)
                        .then(|| alias.to_string())
                })
        })
}

#[cfg(all(test, not(feature = "runtime-provisioning")))]
mod tests {
    use super::selected_runtime_dependency;
    use pnpm_modules_yaml::IncludedDependencies;
    use std::collections::HashSet;

    #[test]
    fn finds_runtime_dependencies_in_selected_snapshots() {
        let lockfile: super::Lockfile = serde_saphyr::from_str(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library: 1.0.0\npackages:\n  library@1.0.0: {}\n  node@runtime:22.0.0: {}\nsnapshots:\n  library@1.0.0:\n    dependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
        )
        .expect("parse lockfile");
        let importer_ids = HashSet::from([".".to_string()]);
        let included = IncludedDependencies {
            dependencies: true,
            dev_dependencies: false,
            optional_dependencies: false,
        };

        assert_eq!(
            selected_runtime_dependency(&lockfile, &importer_ids, included),
            Some("node".to_string())
        );
    }

    #[test]
    fn ignores_optional_runtime_dependencies_when_optionals_are_excluded() {
        let lockfile: super::Lockfile = serde_saphyr::from_str(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library: 1.0.0\npackages:\n  library@1.0.0: {}\n  node@runtime:22.0.0: {}\nsnapshots:\n  library@1.0.0:\n    optionalDependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
        )
        .expect("parse lockfile");
        let importer_ids = HashSet::from([".".to_string()]);
        let included = IncludedDependencies {
            dependencies: true,
            dev_dependencies: false,
            optional_dependencies: false,
        };

        assert_eq!(selected_runtime_dependency(&lockfile, &importer_ids, included), None);
    }
}

impl<'a> MaterializationWorkspace<'a> {
    fn frozen_scope(
        &self,
        lockfile: &Lockfile,
        hoisted_prior: Option<&Lockfile>,
        groups: crate::GroupSelection,
        ignore_manifest_check: bool,
    ) -> FrozenScope<'a> {
        let skipped = crate::SkippedSnapshots::new();
        let closure = if let Some(selected) = self.requested_importer_ids {
            crate::selected_materialization_closure(
                lockfile,
                self.workspace_root,
                selected,
                Some(self.real_importer_ids),
                hoisted_prior,
                &groups,
                &skipped,
            )
        } else {
            let ids = (!ignore_manifest_check).then_some(self.real_importer_ids);
            let initial = initial_materialization_ids(lockfile, ids, None, None);
            crate::materialization_closure(
                lockfile,
                self.workspace_root,
                &initial,
                &groups,
                &skipped,
            )
        };
        let anchors = frozen_project_anchor_ids(
            self.requested_importer_ids,
            self.real_importer_ids,
            &closure,
        );
        let project_manifests =
            anchored_project_manifests(self.project_manifests, self.workspace_root, &anchors);
        let installer_groups = if self.requested_importer_ids.is_some() && hoisted_prior.is_some() {
            crate::GroupSelection::all().linking_as(&groups)
        } else {
            groups.clone()
        };
        FrozenScope { closure, project_manifests, groups, installer_groups }
    }
}

impl MaterializationOutput {
    fn from_frozen(
        frozen_result: pnpm_deps_restorer::InstallFrozenLockfileOutput,
        groups: crate::GroupSelection,
    ) -> Self {
        MaterializationOutput {
            materialized: Materialized {
                hoisted: frozen_result.hoisted,
                ignored_builds: frozen_result.ignored_builds,
                deferred_builds: frozen_result.deferred_builds,

                install_skipped: frozen_result.skipped,
                peer_issue_importer_ids: HashSet::new(),
                fresh_lockfile: None,
                groups: Some(groups),
            },
            store_index_teardown: frozen_result.store_index_teardown,
        }
    }
}
