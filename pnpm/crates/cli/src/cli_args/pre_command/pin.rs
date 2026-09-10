use super::{
    Config, LogEvent, PNPM_VERSION, PackageManager, PackageManagerToSync, PinRoots, PmOnFail,
    PreCommandError, PreCommandInput, ReadEnvLockfile, SwitchInput, SwitchProcessState,
    SwitchSource, SwitchTarget, Value, WantedPackageManager, env_lockfile_sync, global_warn,
    locked_package_manager_version, locked_switch_source, read_env_lockfile, read_manifest_json,
    sanitize_inline, should_persist_package_manager_lockfile, switch_env_root, switch_or_sync,
    version_satisfies, wanted_package_manager,
};

/// What the project's `packageManager` pin asks this invocation to do.
pub(super) enum PinOutcome {
    /// Hand the command over to the pinned pnpm.
    Switch(SwitchTarget),
    /// Keep running, recording the pin in the env lockfile.
    Sync(Option<PackageManagerToSync>),
}

/// The inputs the pin resolution reads.
pub(super) struct PinResolution<'a> {
    pub(super) input: &'a PreCommandInput,
    pub(super) config: &'a Config,
    pub(super) roots: &'a PinRoots,
    pub(super) process_state: SwitchProcessState,
    pub(super) switch: &'a SwitchInput,
}

fn resolve_package_manager_pin(
    resolution: &PinResolution<'_>,
    root_manifest: &Value,
    pm: &WantedPackageManager,
) -> miette::Result<PinOutcome> {
    let PinResolution { input, config, roots, process_state, .. } = *resolution;
    let on_fail = effective_on_fail(config, pm);
    if on_fail == PmOnFail::Ignore {
        return Ok(PinOutcome::Sync(None));
    }
    let switch_wanted = pm.name == "pnpm" && on_fail == PmOnFail::Download;
    // Turning `manage-package-manager-versions` off is the user taking over
    // version selection, so a pin that only asked pnpm to switch has nothing
    // left to report. Corepack is the opposite case: it manages the version
    // and picked the wrong one, so the mismatch is reported rather than
    // switched.
    if switch_wanted && process_state.package_manager_switch_disabled {
        // Which pnpm runs is the user's choice here; which one the lockfile
        // records is still the project's, and a frozen install has to find it
        // there (pnpm/pnpm#14575).
        if input.global {
            return Ok(PinOutcome::Sync(None));
        }
        return Ok(PinOutcome::Sync(env_lockfile_sync(
            config,
            root_manifest,
            roots,
            on_fail,
            ReadEnvLockfile::NotYet,
        )?));
    }
    if switch_wanted && !process_state.executed_by_corepack {
        return switch_or_sync(resolution, root_manifest, on_fail);
    }
    if input.global {
        global_warn(input.emit, "Using --global skips the package manager check for this project");
        return Ok(PinOutcome::Sync(None));
    }
    check_package_manager(pm, on_fail, process_state, input.emit)?;
    Ok(PinOutcome::Sync(env_lockfile_sync(
        config,
        root_manifest,
        roots,
        on_fail,
        ReadEnvLockfile::NotYet,
    )?))
}

/// pnpm's `checkPackageManager`. `on_fail` is already resolved against the
/// `pmOnFail` setting and is never [`PmOnFail::Ignore`] here.
fn check_package_manager(
    pm: &WantedPackageManager,
    on_fail: PmOnFail,
    process_state: SwitchProcessState,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    // `download` reaches this function only when the switch it asks for
    // cannot happen, which leaves the mismatch unresolved — as strict a
    // failure as `error`.
    let should_error = matches!(on_fail, PmOnFail::Error | PmOnFail::Download);
    if pm.name.is_empty() {
        return Ok(());
    }
    if pm.name != "pnpm" {
        let name = sanitize_inline(&pm.name).into_owned();
        if should_error {
            let hint = other_pm_hint(&name);
            return Err(PreCommandError::OtherPmExpected { name, hint }.into());
        }
        global_warn(emit, &format!("This project is configured to use {name}"));
        return Ok(());
    }
    let Some(wanted) = pm.version.as_deref() else {
        return Ok(());
    };
    if version_satisfies(PNPM_VERSION, wanted) {
        return Ok(());
    }
    let (note, hint) = if process_state.executed_by_corepack {
        (COREPACK_NOTE, format!("{COREPACK_PM_HINT_PREFIX}\n{PM_ON_FAIL_HINT}"))
    } else {
        ("", PM_ON_FAIL_HINT.to_string())
    };
    let error =
        PreCommandError::BadPmVersion { wanted: sanitize_inline(wanted).into_owned(), note, hint };
    if should_error {
        return Err(error.into());
    }
    global_warn(emit, &error.to_string());
    Ok(())
}

const PM_ON_FAIL_HINT: &str = r#"If you want to bypass this version check, you can set the "pmOnFail" configuration to "warn" or "ignore" (e.g. via --pm-on-fail=ignore). If using "devEngines.packageManager", you can set its "onFail" to "warn" or "ignore""#;

/// Corepack, not pnpm, picks the running version, so the mismatch survives
/// even the `download` policy that would otherwise switch versions. Both
/// the message and the hint have to say so, or the user is left with a
/// download-on-mismatch contract that silently did not download.
const COREPACK_NOTE: &str = "\nCorepack invoked pnpm with this version, and pnpm does not switch versions when running under corepack.";

const COREPACK_PM_HINT_PREFIX: &str = r#"Align the "packageManager" field in package.json with "devEngines.packageManager", or invoke pnpm directly (without corepack) so it can switch versions automatically."#;

/// What to do about a project that another package manager installs.
///
/// pnpm does not become that package manager just because a project asks
/// for one — typing `pnpm` would then silently run something else — but it
/// can provide it, so the way out is worth naming.
fn other_pm_hint(name: &str) -> String {
    if PackageManager::parse(name).is_none() {
        return format!("pnpm cannot provide {name}. Install it to work on this project.");
    }
    format!(
        r#"Run a one-off command with "pnpm dlx {name} <command>", or link a {name} command that follows this project's pin with "pnpm shim add {name}"."#,
    )
}

pub(super) fn switch_target(
    config: &Config,
    roots: &PinRoots,
    frozen_lockfile: bool,
) -> miette::Result<Option<SwitchTarget>> {
    let Some(manifest) = read_manifest_json(&roots.manifest.join("package.json"))? else {
        return Ok(None);
    };
    let Some(mut pm) = wanted_package_manager(&manifest) else {
        return Ok(None);
    };
    if pm.name != "pnpm" {
        return Ok(None);
    }
    let Some(spec) = pm.version.clone() else {
        return Ok(None);
    };
    let on_fail = effective_on_fail(config, &pm);
    if on_fail != PmOnFail::Download {
        return Ok(None);
    }
    pm.on_fail = Some(on_fail.as_str().to_string());

    // `lockfile: false` opts the project out of `pnpm-lock.yaml`, so the pin
    // has nowhere in the project to persist to. The switch still happens —
    // through the global env below (pnpm/pnpm#14728).
    let persist_lockfile = should_persist_package_manager_lockfile(&pm) && config.lockfile;
    if persist_lockfile
        && let Some(env) = read_env_lockfile(&roots.env)?
        && let Some(version) = locked_package_manager_version(&env, &spec)?
    {
        return Ok(Some(SwitchTarget {
            source: locked_switch_source(env, version, roots, frozen_lockfile),
            spec,
        }));
    }

    let (env_root, frozen_lockfile) =
        switch_env_root(config, roots, frozen_lockfile, persist_lockfile)?;
    Ok(Some(SwitchTarget {
        spec,
        source: SwitchSource::Resolve {
            env_root,
            frozen_lockfile,
            force_resync: false,
            locked_version: None,
        },
    }))
}

fn effective_on_fail(config: &Config, pm: &WantedPackageManager) -> PmOnFail {
    config.pm_on_fail.unwrap_or(match pm.on_fail.as_deref() {
        Some("ignore") => PmOnFail::Ignore,
        Some("warn") => PmOnFail::Warn,
        Some("error") => PmOnFail::Error,
        Some("download") | None => PmOnFail::Download,
        Some(_) => PmOnFail::Download,
    })
}

pub(super) fn resolve_input_pin(
    input: &PreCommandInput,
    config: &Config,
    roots: &PinRoots,
    process_state: SwitchProcessState,
    manifest: Option<&Value>,
    wanted_pm: Option<WantedPackageManager>,
) -> miette::Result<PinOutcome> {
    if !input.skip_pm_handling
        && let Some(root_manifest) = manifest
        && let Some(pm) = wanted_pm
    {
        return resolve_package_manager_pin(
            &PinResolution { input, config, roots, process_state, switch: &input.switch },
            root_manifest,
            &pm,
        );
    }
    Ok(PinOutcome::Sync(None))
}
