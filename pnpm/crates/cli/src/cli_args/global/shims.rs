use super::{
    BTreeMap, BTreeSet, CmdShimHost, Config, Context, GlobalError, GlobalPackageBinSnapshot,
    GlobalShims, HashMap, HashSet, IntoDiagnostic, LinkBinsOptions, PackageBinSource, Path,
    ShimTarget, choose_bins, get_installed_bin_names, install_native_shim,
    link_bins_of_packages_with_excludes, migrate_legacy_shims, read_installed_packages,
    record_package_manager_shims, remove_native_shim, scan_global_packages,
    virtual_shim_bins_to_restore, virtual_shim_owner, virtual_shim_restoration_owners,
};

/// Link `pkgs`' bins into the global bin dir in the shape selected by the
/// `globalShims` record: bins of an enabled providing package become
/// context-aware shims, everything else gets direct shims. The runtime
/// names only count when actually installed through the `runtime:`
/// protocol, so an npm package that happens to be called `node` is not
/// elevated.
pub(super) fn link_global_bins(
    config: &Config,
    pkgs: &[PackageBinSource],
    dependencies: &[(String, String)],
    global_bin_dir: &Path,
    bins_to_skip: &std::collections::HashSet<String>,
) -> miette::Result<()> {
    // A package manager installed globally opts into project-aware
    // dispatch, so it defers to whatever version a project pins and stays
    // the fallback for projects that pin nothing — the arrangement a
    // globally installed runtime already has. The entry is recorded before
    // the split below, so the bins this very run writes are the
    // dispatching flavor.
    let names = pkgs.iter().filter_map(|pkg| pkg.manifest.get("name")?.as_str());
    let newly_enabled = record_package_manager_shims(config, names)?;

    let (direct, context_aware): (Vec<_>, Vec<_>) = pkgs.iter().cloned().partition(|pkg| {
        let name = pkg.manifest.get("name").and_then(serde_json::Value::as_str);
        !name.is_some_and(|name| {
            (config.global_shims.is_enabled(name) || newly_enabled.contains(name))
                && (!pnpm_package_manifest::is_runtime_alias(name)
                    || dependencies
                        .iter()
                        .any(|(alias, spec)| alias == name && spec.starts_with("runtime:")))
        })
    });
    migrate_legacy_shims(global_bin_dir).into_diagnostic().wrap_err("migrate the global shims")?;
    if !direct.is_empty() {
        // A slot turning direct again (its package's shim switched off)
        // must not keep the native shim, which would shadow the direct
        // shim on Windows and hold a stale target everywhere.
        for (command, _) in choose_bins::<CmdShimHost>(&direct, bins_to_skip) {
            remove_native_shim(global_bin_dir, &command.name)
                .into_diagnostic()
                .wrap_err_with(|| format!("remove the stale {} shim", command.name))?;
        }
        link_bins_of_packages_with_excludes::<CmdShimHost>(
            &direct,
            global_bin_dir,
            bins_to_skip,
            &LinkBinsOptions::default(),
        )
        .map_err(miette::Report::new)
        .wrap_err("link direct global package bins")?;
    }
    for (command, _) in choose_bins::<CmdShimHost>(&context_aware, bins_to_skip) {
        install_native_shim(global_bin_dir, &command.name, &ShimTarget::Installed(command.path))
            .into_diagnostic()
            .wrap_err_with(|| format!("install the {} shim", command.name))?;
    }
    Ok(())
}

pub(super) fn unprotected_bin_names(
    groups: &[GlobalPackageBinSnapshot],
    protected: &HashSet<String>,
) -> HashSet<String> {
    groups
        .iter()
        .flat_map(|group| group.bin_names.iter().cloned())
        .filter(|bin| !protected.contains(bin))
        .collect()
}

pub(super) fn check_virtual_shim_conflicts(
    packages: &[PackageBinSource],
    global_bin_dir: &Path,
) -> miette::Result<()> {
    let mut providers_by_bin: HashMap<String, BTreeSet<String>> = HashMap::new();
    for package in packages {
        let package_name =
            package.manifest.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
        for command in pnpm_cmd_shim::get_bins_from_package_manifest::<CmdShimHost>(
            &package.manifest,
            &package.location,
        ) {
            providers_by_bin.entry(command.name).or_default().insert(package_name.to_string());
        }
    }
    if providers_by_bin.is_empty() {
        return Ok(());
    }
    let restoration_owners = virtual_shim_restoration_owners(global_bin_dir)?;
    for (bin, providers) in providers_by_bin {
        let bin_path = global_bin_dir.join(&bin);
        let owner = virtual_shim_owner(&bin_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect global bin at {}", bin_path.display()))?;
        let owner = owner.as_ref().or_else(|| restoration_owners.get(&bin));
        let Some(owner) = owner else { continue };
        if providers.len() == 1 && providers.contains(owner) {
            continue;
        }
        return Err(GlobalError::VirtualShimBinConflict {
            packages: providers.into_iter().collect::<Vec<_>>().join(", "),
            bin,
            shim_package: owner.clone(),
        }
        .into());
    }
    Ok(())
}

pub(super) fn virtual_shims_to_restore(
    groups: &[GlobalPackageBinSnapshot],
    global_bin_dir: &Path,
    protected: &HashSet<String>,
    enabled: &GlobalShims,
) -> miette::Result<BTreeMap<String, BTreeSet<String>>> {
    let mut shims = BTreeMap::<String, BTreeSet<String>>::new();
    for group in groups {
        for package in read_installed_packages(&group.info.install_dir) {
            add_package_shims_to_restore(&package, global_bin_dir, protected, enabled, &mut shims)?;
        }
    }
    Ok(shims)
}

/// Record the shims one replaced package had recorded, minus those the
/// replacement or another group now occupies.
fn add_package_shims_to_restore(
    package: &pnpm_cmd_shim::PackageBinSource,
    global_bin_dir: &Path,
    protected: &HashSet<String>,
    enabled: &GlobalShims,
    shims: &mut BTreeMap<String, BTreeSet<String>>,
) -> miette::Result<()> {
    let Some(package_name) = package.manifest.get("name").and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    if !enabled.is_enabled(package_name) {
        return Ok(());
    }
    let recorded = virtual_shim_bins_to_restore(global_bin_dir, package_name)?
        .into_iter()
        .collect::<HashSet<_>>();
    for command in pnpm_cmd_shim::get_bins_from_package_manifest::<CmdShimHost>(
        &package.manifest,
        &package.location,
    ) {
        if recorded.contains(&command.name) && !protected.contains(&command.name) {
            shims.entry(package_name.to_string()).or_default().insert(command.name);
        }
    }
    Ok(())
}

pub(super) struct ReplacedGlobalBinPlan {
    pub(super) shims_to_restore: BTreeMap<String, BTreeSet<String>>,
    pub(super) affected_bin_names: HashSet<String>,
}

impl ReplacedGlobalBinPlan {
    pub(super) fn restored_bin_names(&self) -> HashSet<String> {
        self.shims_to_restore.values().flatten().cloned().collect()
    }
}

pub(super) fn plan_replaced_global_bins(
    groups: &[GlobalPackageBinSnapshot],
    global_bin_dir: &Path,
    prospective_bins: &HashSet<String>,
    protected_bins: &HashSet<String>,
    enabled: &GlobalShims,
) -> miette::Result<ReplacedGlobalBinPlan> {
    let occupied_bins = prospective_bins.union(protected_bins).cloned().collect::<HashSet<_>>();
    let shims_to_restore =
        virtual_shims_to_restore(groups, global_bin_dir, &occupied_bins, enabled)?;
    let affected_bin_names = groups
        .iter()
        .flat_map(|group| group.bin_names.iter().cloned())
        .filter(|bin| !occupied_bins.contains(bin))
        .collect();
    Ok(ReplacedGlobalBinPlan { shims_to_restore, affected_bin_names })
}

pub(super) fn restore_virtual_shims(
    shims_to_restore: &BTreeMap<String, BTreeSet<String>>,
    global_bin_dir: &Path,
) -> miette::Result<()> {
    for (package, bins) in shims_to_restore {
        for bin in bins {
            install_native_shim(global_bin_dir, bin, &ShimTarget::Virtual(package.clone()))
                .into_diagnostic()
                .wrap_err_with(|| format!("restore the {package} shims"))?;
        }
    }
    Ok(())
}

/// The set of bin names provided by global package groups other than those
/// in `exclude_hashes`.
pub(super) fn bin_names_of_other_groups(
    global_pkg_dir: &Path,
    exclude_hashes: &HashSet<String>,
) -> miette::Result<HashSet<String>> {
    let mut names = HashSet::new();
    for pkg in scan_global_packages(global_pkg_dir).into_diagnostic()? {
        if exclude_hashes.contains(&pkg.hash) {
            continue;
        }
        for bin in get_installed_bin_names(&pkg).map_err(miette::Report::new)? {
            names.insert(bin);
        }
    }
    Ok(names)
}
