use super::{
    CmdShimHost, Config, Context, GlobalInstallTarget, GlobalPackageInfo, GroupActivation,
    GroupInstall, HashSet, IntoDiagnostic, Path, RangeSpecStyle, ReplacedGlobalBinPlan, Reporter,
    SupportedArchitectures, acquire_global_bin_lock, activate_global_install_with_extra_bin_names,
    bin_names_of_other_groups, check_global_bin_conflicts, check_virtual_shim_conflicts,
    cleanup_replaced_global_installs, collect_existing_global_installs, create_global_cache_key,
    create_install_dir, discard_install_dir_on_error, get_actual_bin_names, get_hash_link,
    hash_linked_packages, link_global_bins, pins_for_downgrades, plan_replaced_global_bins,
    read_direct_dependencies, read_installed_packages, registries_with_default,
    replacement_aliases, restore_virtual_shims, run_group_install, should_replace_existing_package,
    snapshot_global_package, update_selectors, warn_global,
};

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
        let aliases = dependencies.iter().map(|(alias, _)| alias.clone()).collect::<Vec<_>>();
        let aliases_to_replace = replacement_aliases(&aliases);
        let _global_bin_lock = discard_install_dir_on_error(
            install_dir,
            acquire_global_bin_lock(self.global_bin_dir),
        )?;

        discard_install_dir_on_error(
            install_dir,
            check_virtual_shim_conflicts(&pkgs, self.global_bin_dir),
        )?;

        let bins_to_skip = discard_install_dir_on_error(
            install_dir,
            check_global_bin_conflicts(
                self.global_pkg_dir,
                self.global_bin_dir,
                &pkgs,
                |existing: &GlobalPackageInfo| {
                    should_replace_existing_package(existing, &aliases, &aliases_to_replace)
                },
            ),
        )?;

        let existing = discard_install_dir_on_error(
            install_dir,
            collect_existing_global_installs(self.global_pkg_dir, &aliases, &aliases_to_replace)
                .wrap_err("scan existing global installs"),
        )?;
        let cache_hash = create_global_cache_key(&aliases, &registries_with_default(config));
        self.activate_group::<Reporter>(&GroupActivation {
            install_dir,
            pkgs: &pkgs,
            dependencies: &dependencies,
            bins_to_skip: &bins_to_skip,
            groups_to_replace: &existing.groups_to_replace,
            protected_bins: &existing.protected_bins,
            hash: &cache_hash,
        })
    }

    /// Reinstall one group for `update -g` and activate it over its own
    /// previous install.
    pub(super) async fn update_group<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        latest: bool,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<()> {
        let install_dir = create_install_dir(self.global_pkg_dir)
            .into_diagnostic()
            .wrap_err("create global install dir")?;
        let pins = Box::pin(pins_for_downgrades::<Reporter>(
            self.base_config,
            self.global_pkg_dir,
            &install_dir,
            pkg,
            latest,
            range_spec_style,
            supported_architectures.clone(),
        ))
        .await?;
        Box::pin(run_group_install::<Reporter>(GroupInstall {
            base_config: self.base_config,
            global_pkg_dir: self.global_pkg_dir,
            install_dir: &install_dir,
            selectors: &update_selectors(&pkg.dependencies, latest, &pins),
            range_spec_style,
            supported_architectures,
            // `update -g` takes no `--allow-build`; the build policy comes
            // from the global `allowBuilds` loaded in `run_group_install`.
            allow_build: &[],
            lockfile_only: false,
        }))
        .await?;

        self.activate_updated_group::<Reporter>(&install_dir, pkg)
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
        discard_install_dir_on_error(
            install_dir,
            check_virtual_shim_conflicts(&pkgs, self.global_bin_dir),
        )?;
        let bins_to_skip = discard_install_dir_on_error(
            install_dir,
            check_global_bin_conflicts(
                self.global_pkg_dir,
                self.global_bin_dir,
                &pkgs,
                |existing: &GlobalPackageInfo| existing.hash == pkg.hash,
            ),
        )?;

        let (group_to_replace, protected) = discard_install_dir_on_error(
            install_dir,
            (|| {
                let group_to_replace = snapshot_global_package(pkg.clone())?;
                let protected = bin_names_of_other_groups(
                    self.global_pkg_dir,
                    &HashSet::from([pkg.hash.clone()]),
                )?;
                Ok::<_, miette::Report>((group_to_replace, protected))
            })()
            .wrap_err("scan global package bin ownership"),
        )?;
        self.activate_group::<Reporter>(&GroupActivation {
            install_dir,
            pkgs: &pkgs,
            dependencies: &dependencies,
            bins_to_skip: &bins_to_skip,
            groups_to_replace: std::slice::from_ref(&group_to_replace),
            protected_bins: &protected,
            hash: &pkg.hash,
        })
    }

    /// Link the group's bins into the global bin directory over the groups
    /// it replaces, then remove those groups.
    fn activate_group<Reporter: self::Reporter>(
        &self,
        activation: &GroupActivation<'_>,
    ) -> miette::Result<()> {
        let replacement_plan = self.replacement_plan(activation)?;
        let hash_link = get_hash_link(self.global_pkg_dir, activation.hash);
        let linked_pkgs = hash_linked_packages(activation.pkgs, activation.install_dir, &hash_link);
        let activated = activate_global_install_with_extra_bin_names::<CmdShimHost>(
            activation.install_dir,
            &hash_link,
            self.global_bin_dir,
            activation.pkgs,
            activation.bins_to_skip,
            &replacement_plan.affected_bin_names,
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
        if let Some(leftover) = &activated.leftover_backup {
            warn_global::<Reporter>(&leftover.to_string());
        }
        if let Some(leftover) = cleanup_replaced_global_installs(
            self.global_pkg_dir,
            self.global_bin_dir,
            activation.groups_to_replace,
            activation.hash,
            &activated.activated_bins,
            activation.protected_bins,
            &replacement_plan.restored_bin_names(),
        )
        .wrap_err("remove existing global installs")?
        {
            warn_global::<Reporter>(&leftover.to_string());
        }
        Ok(())
    }
    fn replacement_plan(
        &self,
        activation: &GroupActivation<'_>,
    ) -> miette::Result<ReplacedGlobalBinPlan> {
        let prospective_bins =
            get_actual_bin_names::<CmdShimHost>(activation.pkgs, activation.bins_to_skip);
        discard_install_dir_on_error(
            activation.install_dir,
            plan_replaced_global_bins(
                activation.groups_to_replace,
                self.global_bin_dir,
                &prospective_bins,
                activation.protected_bins,
                &crate::shim_dispatch::global_shims_setting(),
            ),
        )
    }
}
