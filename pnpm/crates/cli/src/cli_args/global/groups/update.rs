use super::{
    super::GlobalUpdateMode,
    CmdShimHost,
    Config,
    Context,
    GlobalInstallTarget,
    GlobalPackageInfo,
    GroupActivation,
    GroupInstall,
    HashSet,
    IntoDiagnostic,
    Path,
    PathBuf,
    RangeSpecStyle,
    Reporter,
    SupportedArchitectures,
    acquire_global_bin_lock,
    bin_names_of_other_groups,
    create_install_dir,
    discard_install_dir_on_error,
    fs,
    get_actual_bin_names,
    global_approval_store,
    global_group_config,
    is_materialized,
    links_of_other_groups,
    lockfiles_are_equal,
    missing_file_source_warning,
    prompt_approve_install_builds,
    read_direct_dependencies,
    read_installed_packages,
    run_group_install,
    snapshot_global_package,
    warn_global,
};
use crate::cli_args::reporter::EventFilter;

impl GlobalInstallTarget<'_> {
    /// Reinstall each of `groups` for `update -g`, skipping with a warning the
    /// ones [`missing_file_source_warning`] reports. Returns whether at least
    /// one group was checked and none of them changed.
    pub(in crate::cli_args::global) async fn update_groups<Reporter: self::Reporter + 'static>(
        &self,
        groups: &[GlobalPackageInfo],
        mode: GlobalUpdateMode<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<bool> {
        let mut checked = false;
        let mut changed = false;
        for pkg in groups {
            if let Some(warning) = missing_file_source_warning(pkg) {
                warn_global::<Reporter>(warning);
                continue;
            }
            checked = true;
            changed |= self.update_group::<Reporter>(
                pkg,
                mode,
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
        mode: GlobalUpdateMode<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<bool> {
        let (install_dir, selectors) = self.prepare_update_candidate::<Reporter>(
            pkg,
            mode,
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
        mode: GlobalUpdateMode<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<(PathBuf, Vec<String>)> {
        let install_dir = create_install_dir(self.global_pkg_dir)
            .into_diagnostic()
            .wrap_err("create global install dir")?;
        let resolved = Box::pin(self.resolve_update_candidate::<Reporter>(
            pkg,
            &install_dir,
            mode,
            range_spec_style,
            supported_architectures,
        ))
        .await;
        let selectors = discard_install_dir_on_error(&install_dir, resolved)?;
        Ok((install_dir, selectors))
    }

    /// Seed the candidate with the group's manifest and resolve the update into
    /// its lockfile, leaving `node_modules` absent. The selectors it returns are
    /// the ones the materializing install repeats.
    async fn resolve_update_candidate<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        install_dir: &Path,
        mode: GlobalUpdateMode<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<Vec<String>> {
        fs::copy(pkg.install_dir.join("package.json"), install_dir.join("package.json"))
            .into_diagnostic()
            .wrap_err("seed global update manifest")?;
        let _filter = EventFilter::GlobalUpdateResolution.apply();
        self.resolve_candidate_selectors::<Reporter>(
            pkg,
            install_dir,
            mode,
            range_spec_style,
            supported_architectures,
        )
        .await
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
            global_approval_store(self.base_config, self.global_pkg_dir)?,
        )
        .await?;
        let _lock = acquire_global_bin_lock(self.global_bin_dir)?;
        let other_links = links_of_other_groups(
            self.global_pkg_dir,
            &std::collections::BTreeSet::from([pkg.install_dir.as_path()]),
        )?;
        pnpm_package_manager::resync_global_agent_skills(
            active_config,
            &pkg.install_dir,
            &other_links,
        )
        .wrap_err("reconcile skills of the global install")?;
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
        let filter = EventFilter::GlobalUpdateMaterialization.apply();
        let config = Box::pin(run_group_install::<Reporter>(GroupInstall {
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
        }))
        .await?;
        drop(filter);

        self.activate_updated_group::<Reporter>(install_dir, pkg, config)
    }

    fn activate_updated_group<Reporter: self::Reporter>(
        &self,
        install_dir: &Path,
        pkg: &GlobalPackageInfo,
        config: &Config,
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

        let (group_to_replace, protected, retained_bin_names) =
            self.updated_group_replacement(install_dir, pkg, &pkgs, &bins_to_skip)?;
        self.activate_group::<Reporter>(
            &GroupActivation {
                install_dir,
                pkgs: &pkgs,
                dependencies: &dependencies,
                bins_to_skip: &bins_to_skip,
                retained_bin_names: &retained_bin_names,
                groups_to_replace: std::slice::from_ref(&group_to_replace),
                protected_bins: &protected,
                hash: &pkg.hash,
            },
            config,
        )
    }

    fn updated_group_replacement(
        &self,
        install_dir: &Path,
        pkg: &GlobalPackageInfo,
        pkgs: &[super::super::PackageBinSource],
        bins_to_skip: &HashSet<String>,
    ) -> miette::Result<(super::super::GlobalPackageBinSnapshot, HashSet<String>, HashSet<String>)>
    {
        discard_install_dir_on_error(
            install_dir,
            (|| {
                let group_to_replace = snapshot_global_package(pkg.clone())?;
                let retained_bin_names = get_actual_bin_names::<CmdShimHost>(pkgs, bins_to_skip)?;
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
        )
    }
}
