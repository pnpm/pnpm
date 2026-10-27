use super::{
    Config, EntryPoint, EnvLockfile, Invocation, LogEvent, PACKAGE_MANAGER_SWITCH_ENV_VARS,
    PackageManager, PackageManagerToSync, PathBuf, PinRoots, PmOnFail, PreCommandError,
    SwitchProcessState, SwitchSource, SwitchTarget, Value, WantedPackageManager,
    env_lockfile_sync_from_loaded, global_warn, locked_package_manager_version,
    locked_switch_source, sanitize_inline, should_persist_package_manager_lockfile,
    switch_env_root, version_satisfies,
};

/// What [`decide`] chooses for an invocation.
#[derive(Debug)]
pub(crate) enum PinDecision {
    /// Hand the command over to the pinned pnpm.
    Switch(SwitchTarget),
    /// Keep running, recording the pin in the env lockfile at `at`.
    Record { at: PathBuf, package_manager: PackageManagerToSync },
    /// Neither switch nor record.
    Nothing,
}

/// Pre-read manifest and lockfile inputs for [`decide`].
pub(crate) struct PinInput<'a> {
    pub(crate) roots: &'a PinRoots,
    pub(crate) manifest: Option<&'a Value>,
    pub(crate) wanted: Option<&'a WantedPackageManager>,
    pub(crate) locked_env: Option<EnvLockfile>,
    pub(crate) running_version: &'a str,
    pub(crate) process_state: SwitchProcessState,
}

/// Whether `decide` may consult the project's `pnpm-lock.yaml` for `wanted`.
pub(crate) fn should_read_pin_lockfile(
    wanted: Option<&WantedPackageManager>,
    config: &Config,
    invocation: &Invocation,
    running_version: &str,
    process_state: SwitchProcessState,
) -> bool {
    if invocation.global || !config.lockfile {
        return false;
    }
    let Some(pm) = wanted.filter(|pm| pm.name == "pnpm") else {
        return false;
    };
    let Some(spec) = pm.version.as_deref() else {
        return false;
    };
    let on_fail = effective_on_fail(config, pm);
    if on_fail == PmOnFail::Ignore
        || !should_persist_package_manager_lockfile(&WantedPackageManager {
            name: pm.name.clone(),
            version: pm.version.clone(),
            from_dev_engines: pm.from_dev_engines,
            on_fail: Some(on_fail.as_str().to_string()),
        })
    {
        return false;
    }
    let switch_wanted =
        on_fail == PmOnFail::Download && !process_state.package_manager_switch_disabled;
    if can_switch_for_entry_point(invocation.entry_point, switch_wanted, process_state) {
        return true;
    }
    invocation.entry_point != EntryPoint::UnknownFlag
        && (version_satisfies(running_version, spec)
            || crate::cli_args::package_manager::exact_version(spec)
                .is_some_and(|version| version_satisfies(&version, spec)))
}

/// Pure decision over a pre-read [`PinInput`], [`Config`], and [`Invocation`].
/// Performs no filesystem I/O.
pub(crate) fn decide(
    pin: &PinInput<'_>,
    settings: &Config,
    invocation: &Invocation,
) -> miette::Result<PinDecision> {
    if invocation.skip_pm_handling {
        return Ok(PinDecision::Nothing);
    }
    let (Some(root_manifest), Some(pm)) = (pin.manifest, pin.wanted) else {
        return Ok(PinDecision::Nothing);
    };
    let on_fail = effective_on_fail(settings, pm);
    if on_fail == PmOnFail::Ignore {
        return Ok(PinDecision::Nothing);
    }
    decide_enforced_pin(pin, settings, invocation, root_manifest, pm, on_fail)
}

fn decide_enforced_pin(
    pin: &PinInput<'_>,
    settings: &Config,
    invocation: &Invocation,
    root_manifest: &Value,
    pm: &WantedPackageManager,
    on_fail: PmOnFail,
) -> miette::Result<PinDecision> {
    let switch_wanted = pm.name == "pnpm" && on_fail == PmOnFail::Download;
    if switch_wanted && pin.process_state.package_manager_switch_disabled {
        return decide_when_switch_disabled(pin, settings, invocation, root_manifest, on_fail);
    }
    if invocation.global {
        return Ok(decide_global_pin(pin, settings, invocation, pm));
    }
    if can_switch_for_entry_point(invocation.entry_point, switch_wanted, pin.process_state) {
        return decide_switch_or_record(pin, settings, invocation, root_manifest, on_fail);
    }
    if invocation.entry_point == EntryPoint::UnknownFlag {
        return Ok(PinDecision::Nothing);
    }
    check_package_manager(
        pm,
        on_fail,
        pin.process_state,
        pin.running_version,
        invocation.emit(settings),
    )?;
    decide_record(pin, settings, invocation, root_manifest, on_fail)
}

fn decide_global_pin(
    pin: &PinInput<'_>,
    settings: &Config,
    invocation: &Invocation,
    pm: &WantedPackageManager,
) -> PinDecision {
    if invocation.entry_point != EntryPoint::UnknownFlag {
        warn_that_global_skips_the_check(pm, pin.running_version, invocation.emit(settings));
    }
    PinDecision::Nothing
}

fn can_switch_for_entry_point(
    entry_point: EntryPoint,
    switch_wanted: bool,
    process_state: SwitchProcessState,
) -> bool {
    entry_point != EntryPoint::SelfUpdate && switch_wanted && !process_state.executed_by_corepack
}

fn decide_when_switch_disabled(
    pin: &PinInput<'_>,
    settings: &Config,
    invocation: &Invocation,
    root_manifest: &Value,
    on_fail: PmOnFail,
) -> miette::Result<PinDecision> {
    if invocation.global || invocation.entry_point == EntryPoint::UnknownFlag {
        return Ok(PinDecision::Nothing);
    }
    decide_record(pin, settings, invocation, root_manifest, on_fail)
}

fn decide_switch_or_record(
    pin: &PinInput<'_>,
    settings: &Config,
    invocation: &Invocation,
    root_manifest: &Value,
    on_fail: PmOnFail,
) -> miette::Result<PinDecision> {
    let frozen_lockfile = invocation.frozen_lockfile.or(settings.frozen_lockfile).unwrap_or(false);
    let Some(target) = switch_target_from_pin(pin, settings, frozen_lockfile)? else {
        return Ok(PinDecision::Nothing);
    };
    if target.switches_away_from(pin.running_version) {
        return Ok(PinDecision::Switch(target));
    }
    if invocation.entry_point == EntryPoint::UnknownFlag {
        return Ok(PinDecision::Nothing);
    }
    decide_record(pin, settings, invocation, root_manifest, on_fail)
}

fn decide_record(
    pin: &PinInput<'_>,
    settings: &Config,
    invocation: &Invocation,
    root_manifest: &Value,
    on_fail: PmOnFail,
) -> miette::Result<PinDecision> {
    let keep_locked_range_version = invocation.entry_point != EntryPoint::SelfUpdate;
    Ok(
        match env_lockfile_sync_from_loaded(
            settings,
            root_manifest,
            pin.roots,
            on_fail,
            pin.locked_env.as_ref(),
            pin.running_version,
            keep_locked_range_version,
        )? {
            Some(package_manager) => {
                PinDecision::Record { at: pin.roots.env.clone(), package_manager }
            }
            None => PinDecision::Nothing,
        },
    )
}

/// A pin the running pnpm already satisfies has no check to skip.
fn warn_that_global_skips_the_check(
    pm: &WantedPackageManager,
    running_version: &str,
    emit: fn(&LogEvent),
) {
    let running_pnpm_is_pinned = pm.name == "pnpm"
        && pm.version.as_deref().is_none_or(|wanted| version_satisfies(running_version, wanted));
    if !running_pnpm_is_pinned {
        global_warn(emit, "Using --global skips the package manager check for this project");
    }
}

/// pnpm's `checkPackageManager`. `on_fail` is already resolved against the
/// `pmOnFail` setting and is never [`PmOnFail::Ignore`] here.
fn check_package_manager(
    pm: &WantedPackageManager,
    on_fail: PmOnFail,
    process_state: SwitchProcessState,
    running_version: &str,
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
    if version_satisfies(running_version, wanted) {
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

const COREPACK_NOTE: &str = "\nCorepack invoked pnpm with this version, and pnpm does not switch versions when running under corepack.";

const COREPACK_PM_HINT_PREFIX: &str = r#"Align the "packageManager" field in package.json with "devEngines.packageManager", or invoke pnpm directly (without corepack) so it can switch versions automatically."#;

fn other_pm_hint(name: &str) -> String {
    if PackageManager::parse(name).is_none() {
        return format!("pnpm cannot provide {name}. Install it to work on this project.");
    }
    format!(
        r#"Run a one-off command with "pnpm dlx {name} <command>", or link a {name} command that follows this project's pin with "pnpm shim add {name}"."#,
    )
}

pub(super) fn switch_target_from_pin(
    pin: &PinInput<'_>,
    config: &Config,
    frozen_lockfile: bool,
) -> miette::Result<Option<SwitchTarget>> {
    let Some(pm) = pin.wanted.filter(|pm| pm.name == "pnpm") else {
        return Ok(None);
    };
    let Some(spec) = pm.version.clone() else {
        return Ok(None);
    };
    let on_fail = effective_on_fail(config, pm);
    if on_fail != PmOnFail::Download {
        return Ok(None);
    }
    let pm_with_on_fail = WantedPackageManager {
        name: pm.name.clone(),
        version: pm.version.clone(),
        from_dev_engines: pm.from_dev_engines,
        on_fail: Some(on_fail.as_str().to_string()),
    };
    let persist_lockfile =
        should_persist_package_manager_lockfile(&pm_with_on_fail) && config.lockfile;
    if persist_lockfile
        && let Some(env) = pin.locked_env.as_ref()
        && let Some(version) = locked_package_manager_version(env, &spec)?
    {
        return Ok(Some(SwitchTarget {
            source: locked_switch_source(env.clone(), version, pin.roots, frozen_lockfile),
            spec,
        }));
    }
    let (env_root, frozen_lockfile) =
        switch_env_root(config, pin.roots, frozen_lockfile, persist_lockfile)?;
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

pub(super) fn effective_on_fail(config: &Config, pm: &WantedPackageManager) -> PmOnFail {
    match (config.pm_on_fail, pm.on_fail.as_deref()) {
        (Some(on_fail), _) => on_fail,
        (None, Some("ignore")) => PmOnFail::Ignore,
        (None, Some("warn")) => PmOnFail::Warn,
        (None, Some("error")) => PmOnFail::Error,
        (None, _) => PmOnFail::Download,
    }
}

pub(super) fn package_manager_switch_disabled() -> bool {
    PACKAGE_MANAGER_SWITCH_ENV_VARS.into_iter().any(env_var_is_false)
}

fn env_var_is_false(name: &str) -> bool {
    std::env::var_os(name)
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "false" | "0"))
}
