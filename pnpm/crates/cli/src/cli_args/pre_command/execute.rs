use super::{
    Config, Context, EnvLockfileSync, OsString, PNPM_VERSION, PackageManager, PackageManagerCheck,
    Path, PathBuf, PreCommandPlan, SilentReporter, SwitchPlan, SwitchSource, SwitchTarget,
    assert_release_is_installable, config_deps, install_engine_from_env, install_engine_to_store,
    slice, spawn_pnpm,
};

/// Carry out what the pre-command checks planned. Returns whether the command
/// has already been run by a delegated pnpm, in which case the caller is done.
pub(crate) async fn execute_plan(
    plan: PreCommandPlan,
    child_argv: &[OsString],
) -> miette::Result<bool> {
    match plan {
        PreCommandPlan::Switch(plan) => execute_switch(plan, child_argv).await,
        PreCommandPlan::SyncEnvLockfile(sync) => {
            let EnvLockfileSync { config, env_root, package_manager, frozen_lockfile } = sync;
            config_deps::sync_package_manager_dependencies(
                &config,
                &env_root,
                &package_manager.specifier,
                &package_manager.version,
                frozen_lockfile,
                false,
            )
            .await?;
            Ok(false)
        }
    }
}

#[expect(clippy::exit, reason = "delegated pnpm must preserve the child exit code")]
async fn execute_switch(plan: SwitchPlan, child_argv: &[OsString]) -> miette::Result<bool> {
    let SwitchPlan { config, target } = plan;
    let SwitchTarget { spec, source } = target;
    let config = Config::leak(config);
    let Some((version, bin_dir)) = install_switch_target(config, &spec, source).await? else {
        return Ok(false);
    };

    let status =
        spawn_pnpm(slice::from_ref(&bin_dir), child_argv.iter(), PackageManagerCheck::Enabled)
            .wrap_err_with(|| format!("switch pnpm to v{version}"))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(true)
}

/// Install the pinned pnpm and return where its bin landed. `None` when
/// the running pnpm already is the pinned one, so there is nothing to
/// switch to.
async fn install_switch_target(
    config: &'static Config,
    spec: &str,
    source: SwitchSource,
) -> miette::Result<Option<(String, PathBuf)>> {
    match source {
        SwitchSource::LockedEnv { env, version } => {
            if version == PNPM_VERSION {
                return Ok(None);
            }
            assert_release_is_installable(&version)?;
            let bin_dir = Box::pin(install_engine_from_env::<SilentReporter>(
                config,
                PackageManager::Pnpm,
                &env,
                &version,
            ))
            .await?;
            Ok(Some((version, bin_dir)))
        }
        SwitchSource::Resolve { env_root, frozen_lockfile, force_resync, locked_version } => {
            install_resolved_switch_target(
                config,
                spec,
                &env_root,
                frozen_lockfile,
                force_resync,
                locked_version,
            )
            .await
        }
    }
}

/// No switch to perform, but the recorded entries are invalid — heal
/// them now or every later invocation re-resolves over the network. A
/// frozen lockfile cannot be written, and nothing is installed here, so
/// the repair waits for a run that may write.
async fn repair_recorded_entries(
    config: &'static Config,
    env_root: &Path,
    spec: &str,
    version: &str,
    frozen_lockfile: bool,
    force_resync: bool,
) -> miette::Result<()> {
    if !force_resync || frozen_lockfile {
        return Ok(());
    }
    config_deps::sync_package_manager_dependencies(
        config,
        env_root,
        spec,
        version,
        frozen_lockfile,
        true,
    )
    .await?;
    Ok(())
}

async fn install_resolved_switch_target(
    config: &'static Config,
    spec: &str,
    env_root: &Path,
    frozen_lockfile: bool,
    force_resync: bool,
    locked_version: Option<String>,
) -> miette::Result<Option<(String, PathBuf)>> {
    let version = match locked_version.filter(|_| frozen_lockfile) {
        Some(locked) => locked,
        None => {
            config_deps::resolve_engine_version(config, "pnpm", spec)
                .await?
                .ok_or_else(|| miette::miette!(r#"Cannot resolve pnpm version for "{}""#, spec))?
                .version
        }
    };
    if version == PNPM_VERSION {
        repair_recorded_entries(config, env_root, spec, &version, frozen_lockfile, force_resync)
            .await?;
        return Ok(None);
    }
    assert_release_is_installable(&version)?;
    let bin_dir = Box::pin(install_engine_to_store::<SilentReporter>(
        config,
        PackageManager::Pnpm,
        env_root,
        spec,
        &version,
        frozen_lockfile,
        force_resync,
    ))
    .await?;
    Ok(Some((version, bin_dir)))
}
