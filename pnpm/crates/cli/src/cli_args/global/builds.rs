use super::{
    ApproveBuildsArgs, BTreeSet, Config, Context, GlobalError, IgnoredBuildsScan, IntoDiagnostic,
    Path, PathBuf, Reporter, State, clear_decided_ignored_builds, get_automatically_ignored_builds,
    global_group_config, is_subdir, run_rebuild, scan_global_packages, write_approval_settings,
};

pub async fn approve_global_builds<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    args: ApproveBuildsArgs,
) -> miette::Result<()> {
    args.validate()?;
    let global_pkg_dir = base_config.global_pkg_dir.as_ref().ok_or(GlobalError::NoGlobalBinDir)?;
    let (groups, pending) = scan_global_ignored_builds(base_config, global_pkg_dir)?;
    if pending.is_empty() && args.packages.is_empty() {
        println!("There are no packages awaiting approval");
        return Ok(());
    }
    let pending = pending.into_iter().collect::<Vec<_>>();
    let Some(decision) = args.decide::<Reporter>(&pending)? else {
        return Ok(());
    };

    write_approval_settings(global_pkg_dir, &decision)?;
    let mut rebuild_groups = Vec::new();
    for (install_dir, scan) in groups {
        let build_packages: Vec<String> = decision.build_packages
            .iter()
            .filter(|name| {
                scan.names
                    .as_ref()
                    .is_some_and(|names| names.contains(name))
            })
            .cloned()
            .collect();
        clear_decided_ignored_builds(scan.modules_manifest, &scan.modules_dir, &decision)?;
        if !build_packages.is_empty() {
            rebuild_groups.push((install_dir, build_packages));
        }
    }
    rebuild_approved_groups::<Reporter>(base_config, global_pkg_dir, rebuild_groups).await
}

/// Each global group's install dir with its ignored builds.
type IgnoredBuildGroups = Vec<(PathBuf, IgnoredBuildsScan)>;

/// Each global group's ignored builds, with the union of their names. A
/// group whose install dir resolves outside the global packages directory is
/// not this pnpm home's to approve builds for.
fn scan_global_ignored_builds(
    base_config: &Config,
    global_pkg_dir: &Path,
) -> miette::Result<(IgnoredBuildGroups, BTreeSet<String>)> {
    let packages =
        scan_global_packages(global_pkg_dir).into_diagnostic().wrap_err("scan global packages")?;
    let mut groups: IgnoredBuildGroups = Vec::new();
    let mut pending = BTreeSet::new();
    let canonical_global_pkg_dir = (!packages.is_empty())
        .then(|| dunce::canonicalize(global_pkg_dir))
        .transpose()
        .into_diagnostic()
        .wrap_err("resolve the global packages directory")?;
    let contained = packages
        .into_iter()
        .filter(|package| {
            canonical_global_pkg_dir
                .as_ref()
                .is_none_or(|root| is_subdir(root, &package.install_dir))
        });
    for package in contained {
        let config = global_group_config(
            base_config,
            &package.install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?;
        let scan = get_automatically_ignored_builds(&config)?;
        pending.extend(scan.names.iter().flatten().cloned());
        groups.push((package.install_dir, scan));
    }
    Ok((groups, pending))
}

async fn rebuild_approved_groups<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    global_pkg_dir: &Path,
    rebuild_groups: Vec<(PathBuf, Vec<String>)>,
) -> miette::Result<()> {
    for (install_dir, build_packages) in rebuild_groups {
        let config = Config::leak(global_group_config(
            base_config,
            &install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?);
        let state = State::init(install_dir.join("package.json"), config, true)
            .wrap_err("initialize the global approve-builds state")?;
        let selection = crate::cli_args::rebuild::RebuildSelection {
            names: Some(build_packages),
            projects: Vec::new(),
        };
        run_rebuild::<Reporter>(&state, selection, None).await?;
    }
    Ok(())
}
