use super::{
    Config, Context, EnvLockfileSync, InstalledEngine, OsString, PNPM_VERSION, PackageManager,
    PackageManagerCheck, Path, PreCommandPlan, SwitchPlan, SwitchSource, SwitchTarget,
    assert_release_is_installable, config_deps, emit_warning_on_stderr, error_causes, global_warn,
    install_engine_from_env, install_engine_to_store, slice, spawn_pnpm,
};
use crate::cli_args::{
    dlx::exit_unless_success,
    reporter::{CliReporter, EventFilter},
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
            sync_env_lockfile(sync).await?;
            Ok(false)
        }
    }
}

/// A frozen lockfile records nothing new, so the maturity lookup is skipped
/// there: the sync only checks the entry it already has.
async fn sync_env_lockfile(sync: EnvLockfileSync) -> miette::Result<()> {
    let EnvLockfileSync {
        config,
        env_root,
        package_manager,
        frozen_lockfile,
    } = sync;
    let version = if package_manager.running_pnpm_for_range && !frozen_lockfile {
        let Some(version) =
            mature_version_to_record(&config, &package_manager.specifier, &package_manager.version)
                .await
        else {
            return Ok(());
        };
        version
    } else {
        package_manager.version
    };
    config_deps::sync_package_manager_dependencies(
        &config,
        &env_root,
        &package_manager.specifier,
        &version,
        frozen_lockfile,
        false,
    )
    .await?;
    Ok(())
}

async fn execute_switch(plan: SwitchPlan, child_argv: &[OsString]) -> miette::Result<bool> {
    let SwitchPlan { config, target } = plan;
    let SwitchTarget { spec, source } = target;
    let config = Config::leak(config);
    let Some((version, engine)) = install_switch_target(config, &spec, source).await? else {
        return Ok(false);
    };

    let status = engine
        .program(PackageManager::Pnpm)
        .and_then(|program| {
            spawn_pnpm(
                &program,
                slice::from_ref(&engine.bin_dir),
                child_argv.iter(),
                PackageManagerCheck::Enabled,
            )
        })
        .wrap_err_with(|| format!("switch pnpm to v{version}"))?;
    drop(engine);
    // End the way the delegated pnpm did: with its exit code, or with its
    // signal when a signal killed it.
    exit_unless_success(status);
    Ok(true)
}

/// `None` when the lookup fails: nothing is recorded, and the command keeps
/// running on the current pnpm. Recording the running pnpm unchecked could
/// pin a release every other contributor's switch refuses, and the next
/// command retries the lookup.
async fn mature_version_to_record(config: &Config, range: &str, running: &str) -> Option<String> {
    match config_deps::mature_pnpm_version_for_range(config, range, running).await {
        Ok(version) => Some(version),
        Err(error) => {
            global_warn(
                emit_warning_on_stderr,
                &format!(
                    "Skipped recording pnpm v{running} in pnpm-lock.yaml because it could not be checked against minimumReleaseAge: {}",
                    error_causes(&error),
                ),
            );
            None
        }
    }
}

/// Install the pinned pnpm and return it. `None` when the running pnpm
/// already is the pinned one, so there is nothing to switch to.
async fn install_switch_target(
    config: &'static Config,
    spec: &str,
    source: SwitchSource,
) -> miette::Result<Option<(String, InstalledEngine)>> {
    match source {
        SwitchSource::LockedEnv { env, version } => {
            if version == PNPM_VERSION {
                return Ok(None);
            }
            assert_release_is_installable(&version)?;
            let quiet = EventFilter::All.apply();
            let engine = Box::pin(install_engine_from_env::<CliReporter>(
                config,
                PackageManager::Pnpm,
                &env,
                &version,
            ))
            .await?;
            drop(quiet);
            Ok(Some((version, engine)))
        }
        SwitchSource::Resolve {
            env_root,
            frozen_lockfile,
            force_resync,
            locked_version,
        } => {
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
) -> miette::Result<Option<(String, InstalledEngine)>> {
    let version = match locked_version.filter(|_| frozen_lockfile) {
        Some(locked) => locked,
        None => {
            config_deps::resolve_engine_version(config, "pnpm", spec).await?
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
    let _quiet = EventFilter::All.apply();
    let engine = Box::pin(install_engine_to_store::<CliReporter>(
        config,
        PackageManager::Pnpm,
        env_root,
        spec,
        &version,
        frozen_lockfile,
        force_resync,
    ))
    .await?;
    Ok(Some((version, engine)))
}

#[cfg(test)]
mod tests;
