use super::{
    ActivationBinSets, ArtifactCleanupError, CmdShimHost, Config, Context, GlobalInstallTarget,
    GlobalPackageInfo, GlobalUpdateMaterializationReporter, GlobalVersionTarget, GroupActivation,
    GroupInstall, HashSet, IntoDiagnostic, Lockfile, PackageBinSource, Path, PathBuf,
    RangeSpecStyle, ReplacedGlobalBinPlan, Reporter, SupportedArchitectures,
    acquire_global_bin_lock, activate_global_install_with_extra_bin_names,
    bin_names_of_other_groups, check_global_bin_conflicts, check_virtual_shim_conflicts,
    cleanup_replaced_global_installs, collect_existing_global_installs, create_global_cache_key,
    create_install_dir, discard_install_dir_on_error, fs, get_actual_bin_names, get_hash_link,
    global_group_config, hash_linked_packages, link_global_bins, missing_file_source_warning,
    plan_replaced_global_bins, prompt_approve_install_builds, read_direct_dependencies,
    read_installed_packages, registries_with_default, replacement_aliases, restore_virtual_shims,
    run_group_install, should_replace_existing_package, snapshot_global_package, warn_global,
};
use pnpm_modules_yaml::{Host as ModulesHost, read_modules_manifest};

impl GlobalInstallTarget<'_> {
    /// Install one `add -g` group and activate it over the groups it
    /// replaces.
    pub(super) async fn add_group<Reporter: self::Reporter + 'static>(
        &self,
        group: &[String],
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
        allow_build: &[String],
    ) -> miette::Result<()> {
        let install_dir = create_install_dir(self.global_pkg_dir)
            .into_diagnostic()
            .wrap_err("create global install dir")?;
        let config = Box::pin(run_group_install::<Reporter>(GroupInstall {
            base_config: self.base_config,
            global_pkg_dir: self.global_pkg_dir,
            install_dir: &install_dir,
            selectors: group,
            range_spec_style,
            supported_architectures,
            allow_build,
            lockfile_only: false,
        }))
        .await?;

        self.activate_added_group::<Reporter>(&install_dir, config)
    }

    fn activate_added_group<Reporter: self::Reporter>(
        &self,
        install_dir: &Path,
        config: &Config,
    ) -> miette::Result<()> {
        let pkgs = read_installed_packages(install_dir);
        let dependencies = read_direct_dependencies(install_dir);
        let aliases = dependency_aliases(&dependencies);
        let aliases_to_replace = replacement_aliases(&aliases);
        let _global_bin_lock = discard_install_dir_on_error(
            install_dir,
            acquire_global_bin_lock(self.global_bin_dir),
        )?;

        let bins_to_skip = self.check_activation_conflicts(install_dir, &pkgs, |existing| {
            should_replace_existing_package(existing, &aliases, &aliases_to_replace)
        })?;

        let retained_bin_names = discard_install_dir_on_error(
            install_dir,
            get_actual_bin_names::<CmdShimHost>(&pkgs, &bins_to_skip),
        )?;
        let existing = discard_install_dir_on_error(
            install_dir,
            collect_existing_global_installs(
                self.global_pkg_dir,
                &aliases,
                &aliases_to_replace,
                &retained_bin_names,
            )
            .wrap_err("scan existing global installs"),
        )?;
        let cache_hash = create_global_cache_key(&aliases, &registries_with_default(config));
        self.activate_group::<Reporter>(&GroupActivation {
            install_dir,
            pkgs: &pkgs,
            dependencies: &dependencies,
            bins_to_skip: &bins_to_skip,
            retained_bin_names: &retained_bin_names,
            groups_to_replace: &existing.groups_to_replace,
            protected_bins: &existing.protected_bins,
            hash: &cache_hash,
        })
    }

    /// Reinstall each of `groups` for `update -g`, skipping with a warning the
    /// ones [`missing_file_source_warning`] reports. Returns whether at least
    /// one group was checked and none of them changed.
    pub(super) async fn update_groups<Reporter: self::Reporter + 'static>(
        &self,
        groups: &[GlobalPackageInfo],
        version_target: GlobalVersionTarget<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<bool> {
        let mut checked = false;
        let mut changed = false;
        for pkg in groups {
            if let Some(warning) = missing_file_source_warning(pkg) {
                warn_global::<Reporter>(&warning);
                continue;
            }
            checked = true;
            changed |= self.update_group::<Reporter>(
                pkg,
                version_target,
                range_spec_style,
                supported_architectures.clone(),
            )
            .await?;
        }
        Ok(checked && !changed)
    }

    /// Reinstall one group for `update -g` and activate it over its own
    /// previous install.
    async fn update_group<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        version_target: GlobalVersionTarget<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<bool> {
        let (install_dir, selectors) = self.prepare_update_candidate::<Reporter>(
            pkg,
            version_target,
            range_spec_style,
            supported_architectures.clone(),
        )
        .await?;

        if self.discard_unchanged_update::<Reporter>(
            pkg,
            &install_dir,
            supported_architectures.clone(),
        )
        .await?
        {
            return Ok(false);
        }

        self.materialize_update::<Reporter>(
            pkg,
            &install_dir,
            &selectors,
            range_spec_style,
            supported_architectures,
        )
        .await?;
        Ok(true)
    }

    async fn prepare_update_candidate<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        version_target: GlobalVersionTarget<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<(PathBuf, Vec<String>)> {
        let install_dir = create_install_dir(self.global_pkg_dir)
            .into_diagnostic()
            .wrap_err("create global install dir")?;
        let resolved = Box::pin(self.resolve_update_candidate::<Reporter>(
            pkg,
            &install_dir,
            version_target,
            range_spec_style,
            supported_architectures,
        ))
        .await;
        let selectors = discard_install_dir_on_error(&install_dir, resolved)?;
        Ok((install_dir, selectors))
    }

    async fn discard_unchanged_update<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        install_dir: &Path,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<bool> {
        let unchanged =
            is_materialized(&pkg.install_dir) && lockfiles_are_equal(&pkg.install_dir, install_dir);
        if !unchanged {
            return Ok(false);
        }
        fs::remove_dir_all(install_dir)
            .into_diagnostic()
            .wrap_err("remove unchanged global install candidate")?;
        let active_config = Config::leak(global_group_config(
            self.base_config,
            &pkg.install_dir,
            self.global_pkg_dir,
            supported_architectures,
        )?);
        prompt_approve_install_builds::<Reporter>(
            active_config,
            &pkg.install_dir,
            self.global_pkg_dir,
        )
        .await?;
        Ok(true)
    }

    async fn materialize_update<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        install_dir: &Path,
        selectors: &[String],
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<()> {
        Box::pin(run_group_install::<GlobalUpdateMaterializationReporter<Reporter>>(
            GroupInstall {
                base_config: self.base_config,
                global_pkg_dir: self.global_pkg_dir,
                install_dir,
                selectors,
                range_spec_style,
                supported_architectures,
                // `update -g` takes no `--allow-build`; the build policy comes
                // from the global `allowBuilds` loaded in `run_group_install`.
                allow_build: &[],
                lockfile_only: false,
            },
        ))
        .await?;

        self.activate_updated_group::<Reporter>(install_dir, pkg)
    }

    fn activate_updated_group<Reporter: self::Reporter>(
        &self,
        install_dir: &Path,
        pkg: &GlobalPackageInfo,
    ) -> miette::Result<()> {
        let pkgs = read_installed_packages(install_dir);
        let dependencies = read_direct_dependencies(install_dir);
        let _global_bin_lock = discard_install_dir_on_error(
            install_dir,
            acquire_global_bin_lock(self.global_bin_dir),
        )?;
        let bins_to_skip = self.check_activation_conflicts(install_dir, &pkgs, |existing| {
            existing.hash == pkg.hash
        })?;

        let (group_to_replace, protected, retained_bin_names) = discard_install_dir_on_error(
            install_dir,
            (|| {
                let group_to_replace = snapshot_global_package(pkg.clone())?;
                let retained_bin_names = get_actual_bin_names::<CmdShimHost>(&pkgs, &bins_to_skip)?;
                let bin_names_to_protect = group_to_replace.bin_names
                    .iter()
                    .filter(|bin| !retained_bin_names.contains(*bin))
                    .cloned()
                    .collect();
                let protected = bin_names_of_other_groups(
                    self.global_pkg_dir,
                    &HashSet::from([pkg.hash.clone()]),
                    &bin_names_to_protect,
                )?;
                Ok::<_, miette::Report>((group_to_replace, protected, retained_bin_names))
            })()
            .wrap_err("scan global package bin ownership"),
        )?;
        self.activate_group::<Reporter>(&GroupActivation {
            install_dir,
            pkgs: &pkgs,
            dependencies: &dependencies,
            bins_to_skip: &bins_to_skip,
            retained_bin_names: &retained_bin_names,
            groups_to_replace: std::slice::from_ref(&group_to_replace),
            protected_bins: &protected,
            hash: &pkg.hash,
        })
    }

    fn check_activation_conflicts(
        &self,
        install_dir: &Path,
        pkgs: &[PackageBinSource],
        should_replace: impl Fn(&GlobalPackageInfo) -> bool,
    ) -> miette::Result<HashSet<String>> {
        discard_install_dir_on_error(
            install_dir,
            check_virtual_shim_conflicts(pkgs, self.global_bin_dir),
        )?;
        discard_install_dir_on_error(
            install_dir,
            check_global_bin_conflicts(
                self.global_pkg_dir,
                self.global_bin_dir,
                pkgs,
                should_replace,
            ),
        )
    }

    /// Link the group's bins into the global bin directory over the groups
    /// it replaces, then remove those groups.
    fn activate_group<Reporter: self::Reporter>(
        &self,
        activation: &GroupActivation<'_>,
    ) -> miette::Result<()> {
        let replacement_plan = self.replacement_plan(activation, activation.retained_bin_names)?;
        let hash_link = get_hash_link(self.global_pkg_dir, activation.hash);
        let linked_pkgs = hash_linked_packages(activation.pkgs, activation.install_dir, &hash_link);
        let activated = activate_global_install_with_extra_bin_names::<CmdShimHost>(
            activation.install_dir,
            &hash_link,
            self.global_bin_dir,
            activation.pkgs,
            activation.bins_to_skip,
            ActivationBinSets {
                extra: &replacement_plan.affected_bin_names,
                required: activation.retained_bin_names,
            },
            || {
                link_global_bins(
                    self.base_config,
                    &linked_pkgs,
                    activation.dependencies,
                    self.global_bin_dir,
                    activation.bins_to_skip,
                )?;
                restore_virtual_shims(&replacement_plan.shims_to_restore, self.global_bin_dir)
            },
        )
        .wrap_err("activate global install")?;
        warn_on_leftover::<Reporter>(activated.leftover_backup.as_ref());
        let leftover = cleanup_replaced_global_installs(
            self.global_pkg_dir,
            self.global_bin_dir,
            activation.groups_to_replace,
            activation.hash,
            &activated.activated_bins,
            activation.protected_bins,
            &replacement_plan.restored_bin_names(),
        )
        .wrap_err("remove existing global installs")?;
        warn_on_leftover::<Reporter>(leftover.as_ref());
        Ok(())
    }
    fn replacement_plan(
        &self,
        activation: &GroupActivation<'_>,
        prospective_bins: &HashSet<String>,
    ) -> miette::Result<ReplacedGlobalBinPlan> {
        discard_install_dir_on_error(
            activation.install_dir,
            plan_replaced_global_bins(
                activation.groups_to_replace,
                self.global_bin_dir,
                prospective_bins,
                activation.protected_bins,
                &crate::shim_dispatch::global_shims_setting(),
            ),
        )
    }
}

/// The modules manifest is what an install leaves behind, so it answers what
/// equal lockfiles cannot: whether the tree they describe is still on disk.
fn is_materialized(install_dir: &Path) -> bool {
    read_modules_manifest::<ModulesHost>(&install_dir.join("node_modules"))
        .is_ok_and(|manifest| manifest.is_some())
}

fn lockfiles_are_equal(active_dir: &Path, candidate_dir: &Path) -> bool {
    let Ok(Some(active)) = Lockfile::load_from_path(&active_dir.join(Lockfile::FILE_NAME)) else {
        return false;
    };
    let Ok(Some(candidate)) = Lockfile::load_from_path(&candidate_dir.join(Lockfile::FILE_NAME))
    else {
        return false;
    };
    active == candidate
}

fn warn_on_leftover<Reporter: self::Reporter>(leftover: Option<&ArtifactCleanupError>) {
    if let Some(leftover) = leftover {
        warn_global::<Reporter>(&leftover.to_string());
    }
}

fn dependency_aliases(dependencies: &[(String, String)]) -> Vec<String> {
    dependencies
        .iter()
        .map(|(alias, _)| alias.clone())
        .collect()
}
