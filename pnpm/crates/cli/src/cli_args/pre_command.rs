//! pnpm's pre-command handling: before a command runs, the pinned package
//! manager and the pinned runtimes are reconciled with what is actually
//! running. A pnpm pin with `onFail: "download"` switches the CLI to the
//! wanted version; every other pin is validated in place and fails or warns.
//!
//! Mirrors the block at the top of pnpm's `pnpm/src/main.ts`.

mod system_runtime_version;

use super::{
    cli_command::{CliArgs, CliCommand},
    config::ConfigSubcommand,
    package_manager::{
        PACKAGE_MANAGER_SWITCH_ENV_VARS, WantedPackageManager, read_manifest_json,
        should_persist_package_manager_lockfile, version_satisfies, wanted_package_manager,
    },
    reporter::reporter_emit,
    sanitize::sanitize_inline,
    self_update::install_pnpm::{assert_release_is_installable, pnpm_package_to_install},
    with::{
        PackageManagerCheck,
        install_pnpm_to_store::{install_pnpm_from_env, install_pnpm_to_store},
        spawn_pnpm,
    },
};
use crate::{config_deps, config_overrides::ConfigOverrides};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::{Config, Host, PNPM_VERSION, PmOnFail};
use pacquet_default_reporter::DefaultReporter;
use pacquet_lockfile::{EnvLockfile, LockfileResolution, PackageKey, PackageMetadata, VersionPart};
use pacquet_package_manifest::{apply_runtime_on_fail_override, is_runtime_alias};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter, SilentReporter};
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};
use system_runtime_version::system_runtime_version;

/// Run the pre-command checks and return the version switch they ask for,
/// if any. The checks themselves report through `args.reporter` and fail
/// the command by returning an error.
pub(crate) fn pre_command_plan(
    args: &CliArgs,
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<SwitchPlan>> {
    if should_skip_command(&args.command) {
        return Ok(None);
    }
    pre_command_plan_from_input(
        &PreCommandInput {
            switch: SwitchInput::from_cli_args(args),
            global: is_global(&args.command),
            check_runtimes: true,
            emit: reporter_emit(args.reporter),
        },
        config_overrides,
        SwitchProcessState::current(),
    )
}

/// The `pnpm --version` path, which clap answers before a command is
/// parsed. pnpm checks the package manager there too, but skips the runtime
/// checks — printing the version must work in a project whose runtime pin
/// the system cannot satisfy.
pub(crate) fn pre_command_plan_for_version_flag(
    argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<SwitchPlan>> {
    pre_command_plan_from_input(
        &PreCommandInput {
            switch: SwitchInput::from_version_argv(argv),
            global: false,
            check_runtimes: false,
            emit: DefaultReporter::emit,
        },
        config_overrides,
        SwitchProcessState::current(),
    )
}

#[expect(clippy::exit, reason = "delegated pnpm must preserve the child exit code")]
pub(crate) async fn execute_switch(
    plan: SwitchPlan,
    child_argv: &[OsString],
) -> miette::Result<bool> {
    let SwitchPlan { config, target } = plan;
    let SwitchTarget { spec, source } = target;
    let config = Config::leak(config);
    let (version, bin_dir) = match source {
        SwitchSource::LockedEnv { env, version } => {
            if version == PNPM_VERSION {
                return Ok(false);
            }
            assert_release_is_installable(&version)?;
            let bin_dir =
                Box::pin(install_pnpm_from_env::<SilentReporter>(config, &env, &version)).await?;
            (version, bin_dir)
        }
        SwitchSource::Resolve { env_root } => {
            let resolved = config_deps::resolve_pnpm_version(config, &spec)
                .await?
                .ok_or_else(|| miette::miette!(r#"Cannot resolve pnpm version for "{}""#, spec))?;
            if resolved.version == PNPM_VERSION {
                return Ok(false);
            }
            assert_release_is_installable(&resolved.version)?;
            let bin_dir = Box::pin(install_pnpm_to_store::<SilentReporter>(
                config,
                &env_root,
                &spec,
                &resolved.version,
            ))
            .await?;
            (resolved.version, bin_dir)
        }
    };

    let status = spawn_pnpm(&bin_dir, child_argv.iter(), PackageManagerCheck::Enabled)
        .wrap_err_with(|| format!("switch pnpm to v{version}"))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(true)
}

fn pre_command_plan_from_input(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    process_state: SwitchProcessState,
) -> miette::Result<Option<SwitchPlan>> {
    let switch = &input.switch;
    if switch.command.as_deref().is_some_and(should_skip_command_name) {
        return Ok(None);
    }
    let dir = dunce::canonicalize(&switch.dir).into_diagnostic().wrap_err_with(|| {
        format!("canonicalizing the `--dir` argument: {}", switch.dir.display())
    })?;
    let mut config =
        Config { npmrc_auth_file: switch.npmrc_auth_file.clone(), ..Config::default() }
            .current::<Host>(&dir)
            .map_err(miette::Report::new)
            .wrap_err("load configuration")?;
    config_overrides.apply(&mut config);

    let root_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.clone());
    let manifest = read_manifest_json(&root_dir.join("package.json"))?;

    if let Some(pm) = manifest.as_ref().and_then(wanted_package_manager) {
        let on_fail = effective_on_fail(&config, &pm);
        let switch_wanted = pm.name == "pnpm" && on_fail == PmOnFail::Download;
        // Turning `manage-package-manager-versions` off is the user taking
        // over version selection, so a pin that only asked pnpm to switch has
        // nothing left to report. Corepack is the opposite case: it manages
        // the version and picked the wrong one, so the mismatch is reported
        // rather than switched.
        let unmanaged_pin = switch_wanted && process_state.package_manager_switch_disabled;
        if on_fail != PmOnFail::Ignore && !unmanaged_pin {
            if switch_wanted && !process_state.executed_by_corepack {
                if let Some(target) = switch_target(&config, &root_dir)?
                    && !version_satisfies(PNPM_VERSION, &target.spec)
                {
                    return Ok(Some(SwitchPlan { config, target }));
                }
            } else if input.global {
                global_warn(
                    input.emit,
                    "Using --global skips the package manager check for this project",
                );
            } else {
                check_package_manager(&pm, on_fail, process_state, input.emit)?;
            }
        }
    }

    if input.check_runtimes
        && !input.global
        && let Some(manifest) = manifest
    {
        check_runtimes(manifest, &config, input.emit)?;
    }
    Ok(None)
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
            return Err(PreCommandError::OtherPmExpected { name }.into());
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

/// pnpm's `getWantedRuntimes` + `checkRuntime`: validate every runtime the
/// root manifest pins against the runtime installed on the system.
fn check_runtimes(mut manifest: Value, config: &Config, emit: fn(&LogEvent)) -> miette::Result<()> {
    if let Some(runtime_on_fail) = config.runtime_on_fail {
        apply_runtime_on_fail_override(&mut manifest, runtime_on_fail.as_str());
    }
    // `devEngines.runtime` wins over `engines.runtime`: the first entry seen
    // for a runtime is the one that gets checked.
    let mut checked = HashSet::new();
    for engines_field in ["devEngines", "engines"] {
        let Some(runtime_entry) =
            manifest.get(engines_field).and_then(|engines| engines.get("runtime"))
        else {
            continue;
        };
        let runtimes: &[Value] = match runtime_entry {
            Value::Array(runtimes) => runtimes.as_slice(),
            runtime @ Value::Object(_) => std::slice::from_ref(runtime),
            _ => continue,
        };
        for runtime in runtimes {
            let Some(name) = runtime.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !is_runtime_alias(name) || !checked.insert(name.to_string()) {
                continue;
            }
            check_runtime(runtime, name, emit)?;
        }
    }
    Ok(())
}

fn check_runtime(runtime: &Value, name: &str, emit: fn(&LogEvent)) -> miette::Result<()> {
    let on_fail = runtime.get("onFail").and_then(Value::as_str);
    if matches!(on_fail, None | Some("ignore" | "download")) {
        return Ok(());
    }
    let display_name = runtime_display_name(name);
    let wanted_range = runtime.get("version").and_then(Value::as_str).map(str::trim);
    let Some(wanted_range) = wanted_range.filter(|range| !range.is_empty()) else {
        return fail_runtime_check(
            on_fail,
            &format!(
                "This project requires a {display_name} runtime but does not specify a version range",
            ),
            emit,
        );
    };
    if node_semver::Range::parse(wanted_range).is_err() {
        return fail_runtime_check(
            on_fail,
            &format!(
                "This project requires an invalid {display_name} version range: {wanted_range}",
            ),
            emit,
        );
    }
    let Some(current_version) = system_runtime_version(name) else {
        return fail_runtime_check(
            on_fail,
            &format!(
                "This project requires {display_name} {wanted_range}, but {display_name} was not found on the system",
            ),
            emit,
        );
    };
    if version_satisfies(&current_version, wanted_range) {
        return Ok(());
    }
    fail_runtime_check(
        on_fail,
        &format!(
            "This project requires {display_name} {wanted_range}. Your current {display_name} is v{current_version}",
        ),
        emit,
    )
}

/// Every `onFail` other than `error` — including a value pnpm does not
/// define — degrades to a warning.
fn fail_runtime_check(
    on_fail: Option<&str>,
    message: &str,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    if on_fail == Some("error") {
        let message = sanitize_inline(message).into_owned();
        return Err(PreCommandError::BadRuntimeVersion { message }.into());
    }
    global_warn(emit, message);
    Ok(())
}

fn runtime_display_name(runtime: &str) -> &'static str {
    match runtime {
        "deno" => "Deno",
        "bun" => "Bun",
        _ => "Node.js",
    }
}

/// Every warning here quotes the project's manifest, which is untrusted
/// input in a repository the user has only cloned, so control characters
/// are stripped before the message reaches the terminal.
fn global_warn(emit: fn(&LogEvent), message: &str) {
    let message = sanitize_inline(message).into_owned();
    emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
}

const PM_ON_FAIL_HINT: &str = r#"If you want to bypass this version check, you can set the "pmOnFail" configuration to "warn" or "ignore" (e.g. via --pm-on-fail=ignore). If using "devEngines.packageManager", you can set its "onFail" to "warn" or "ignore""#;

/// Corepack, not pnpm, picks the running version, so the mismatch survives
/// even the `download` policy that would otherwise switch versions. Both
/// the message and the hint have to say so, or the user is left with a
/// download-on-mismatch contract that silently did not download.
const COREPACK_NOTE: &str = "\nCorepack invoked pnpm with this version, and pnpm does not switch versions when running under corepack.";

const COREPACK_PM_HINT_PREFIX: &str = r#"Align the "packageManager" field in package.json with "devEngines.packageManager", or invoke pnpm directly (without corepack) so it can switch versions automatically."#;

const RUNTIME_ON_FAIL_HINT: &str = r#"If you want to bypass this version check, set "runtimeOnFail" to "warn" or "ignore" (e.g. via --runtime-on-fail=ignore), or set "devEngines.runtime.onFail"/"engines.runtime.onFail" to "warn" or "ignore""#;

#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum PreCommandError {
    #[display("This project is configured to use {name}")]
    #[diagnostic(code(ERR_PNPM_OTHER_PM_EXPECTED))]
    OtherPmExpected { name: String },

    #[display(
        "This project is configured to use {wanted} of pnpm. Your current pnpm is v{PNPM_VERSION}{note}"
    )]
    #[diagnostic(code(ERR_PNPM_BAD_PM_VERSION), help("{hint}"))]
    BadPmVersion { wanted: String, note: &'static str, hint: String },

    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_BAD_RUNTIME_VERSION), help("{RUNTIME_ON_FAIL_HINT}"))]
    BadRuntimeVersion { message: String },
}

struct PreCommandInput {
    switch: SwitchInput,
    global: bool,
    check_runtimes: bool,
    emit: fn(&LogEvent),
}

/// pnpm treats `--global` as an opt-out of the project's package manager
/// and runtime pins — a global install does not belong to the project.
fn is_global(command: &CliCommand) -> bool {
    match command {
        CliCommand::Add(args) => args.global,
        CliCommand::ApproveBuilds(args) => args.global,
        CliCommand::Bin(args) => args.global,
        CliCommand::Config(args) => match &args.command {
            ConfigSubcommand::Set(args) => args.flags.global,
            ConfigSubcommand::Get(args) => args.flags.global,
            ConfigSubcommand::Delete(args) => args.flags.global,
            ConfigSubcommand::List(args) => args.flags.global,
        },
        CliCommand::List(args) | CliCommand::Ll(args) => args.global,
        CliCommand::Outdated(args) => args.global,
        CliCommand::Prefix(args) => args.global,
        CliCommand::Remove(args) => args.global,
        CliCommand::Root(args) => args.global,
        CliCommand::Runtime(args) => args.global,
        CliCommand::Update(args) => args.global,
        // `pnpm link` with no arguments links the current project into the
        // global directory.
        CliCommand::Link(args) => args.package_paths.is_empty(),
        _ => false,
    }
}

fn switch_target(config: &Config, root_dir: &Path) -> miette::Result<Option<SwitchTarget>> {
    let Some(manifest) = read_manifest_json(&root_dir.join("package.json"))? else {
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
    pm.on_fail = Some(on_fail_str(on_fail).to_string());

    let persist_lockfile = should_persist_package_manager_lockfile(&pm);
    if persist_lockfile
        && let Some(env) =
            EnvLockfile::read(root_dir).map_err(miette::Report::new).wrap_err_with(|| {
                format!("read the package-manager env lockfile in {}", root_dir.display())
            })?
        && let Some(version) = locked_package_manager_version(&env, &spec)?
    {
        return Ok(Some(SwitchTarget { spec, source: SwitchSource::LockedEnv { env, version } }));
    }

    let env_root = if persist_lockfile {
        root_dir.to_path_buf()
    } else {
        config.global_pkg_dir.clone().ok_or_else(|| {
            miette::miette!(
                r#"Unable to find the global packages directory. Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable."#,
            )
        })?
    };
    Ok(Some(SwitchTarget { spec, source: SwitchSource::Resolve { env_root } }))
}

fn locked_package_manager_version(
    env: &EnvLockfile,
    wanted_range: &str,
) -> miette::Result<Option<String>> {
    let Some(version) = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
        .and_then(|dependencies| dependencies.get("pnpm"))
        .map(|dependency| dependency.version.clone())
    else {
        return Ok(None);
    };
    if !version_satisfies(&version, wanted_range) {
        return Ok(None);
    }
    if !package_manager_dependencies_are_resolved(env, &version) {
        return Ok(None);
    }
    assert_package_manager_lockfile_uses_registry_resolutions(env)?;
    Ok(Some(version))
}

fn package_manager_dependencies_are_resolved(env: &EnvLockfile, version: &str) -> bool {
    let Some(dependencies) = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
    else {
        return false;
    };
    if dependencies.get("pnpm").is_none_or(|dep| dep.version != version) {
        return false;
    }
    let wrapper_pkg_name = pnpm_package_to_install(version).name;
    wrapper_pkg_name == "pnpm"
        || dependencies.get(wrapper_pkg_name).is_some_and(|dep| dep.version == version)
}

fn assert_package_manager_lockfile_uses_registry_resolutions(
    env: &EnvLockfile,
) -> miette::Result<()> {
    let Some(package_manager_dependencies) = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
    else {
        return Err(miette::miette!(
            "The packageManager dependencies were not found in pnpm-lock.yaml"
        ));
    };

    let mut visited = HashSet::new();
    let mut pending = Vec::with_capacity(package_manager_dependencies.len());
    for (name, dependency) in package_manager_dependencies {
        let key = format!("{name}@{}", dependency.version)
            .parse::<PackageKey>()
            .map_err(|_| invalid_package_manager_lockfile(name))?;
        pending.push(key);
    }

    while let Some(key) = pending.pop() {
        if !visited.insert(key.clone()) {
            continue;
        }

        let package_key = key.without_peer();
        let package_info =
            env.packages.get(&package_key).ok_or_else(|| invalid_package_manager_lockfile(&key))?;
        let snapshot =
            env.snapshots.get(&key).ok_or_else(|| invalid_package_manager_lockfile(&key))?;

        assert_registry_package_path(&key, package_info)?;
        assert_integrity_only_resolution(&key, &package_info.resolution)?;

        for dependencies in
            [&snapshot.dependencies, &snapshot.optional_dependencies].into_iter().flatten()
        {
            for (name, reference) in dependencies {
                let next_key = reference
                    .resolve(name)
                    .ok_or_else(|| invalid_package_manager_lockfile(&key))?;
                pending.push(next_key);
            }
        }
    }
    Ok(())
}

fn assert_registry_package_path(
    key: &PackageKey,
    package_info: &PackageMetadata,
) -> miette::Result<()> {
    if key.suffix.prefix() != pacquet_lockfile::Prefix::None
        || !matches!(key.suffix.version(), VersionPart::Semver(_))
    {
        return Err(invalid_package_manager_lockfile(key));
    }
    if let Some(version) = &package_info.version
        && version != &key.suffix.without_peer().to_string()
    {
        return Err(invalid_package_manager_lockfile(key));
    }
    Ok(())
}

fn assert_integrity_only_resolution(
    key: &PackageKey,
    resolution: &LockfileResolution,
) -> miette::Result<()> {
    match resolution {
        LockfileResolution::Registry(resolution)
            if !resolution.integrity.to_string().is_empty() =>
        {
            Ok(())
        }
        LockfileResolution::Registry(_)
        | LockfileResolution::Tarball(_)
        | LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => Err(invalid_package_manager_lockfile(key)),
    }
}

fn invalid_package_manager_lockfile(dep_path: impl std::fmt::Display) -> miette::Report {
    miette::miette!(
        r#"The packageManager dependency "{}" in pnpm-lock.yaml must use a registry package path and an integrity-only resolution"#,
        dep_path,
    )
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

fn on_fail_str(on_fail: PmOnFail) -> &'static str {
    match on_fail {
        PmOnFail::Download => "download",
        PmOnFail::Error => "error",
        PmOnFail::Warn => "warn",
        PmOnFail::Ignore => "ignore",
    }
}

/// The commands pnpm marks with `skipPackageManagerCheck`, plus `setup` —
/// they either predate the project's pins (`setup`), inspect pnpm itself
/// (`store`, `doctor`, and so on), or run something that is not the project
/// (`dlx`).
fn should_skip_command(command: &CliCommand) -> bool {
    matches!(
        command,
        CliCommand::CatFile(_)
            | CliCommand::CatIndex(_)
            | CliCommand::Completion(_)
            | CliCommand::CompletionServer(_)
            | CliCommand::Dlx(_)
            | CliCommand::Doctor(_)
            | CliCommand::FindHash(_)
            | CliCommand::Runtime(_)
            | CliCommand::SelfUpdate(_)
            | CliCommand::Setup(_)
            | CliCommand::Store(_)
            | CliCommand::With(_),
    )
}

fn should_skip_command_name(command: &str) -> bool {
    matches!(
        command,
        "cat-file"
            | "cat-index"
            | "completion"
            | "completion-server"
            | "dlx"
            | "doctor"
            | "env"
            | "find-hash"
            | "runtime"
            | "rt"
            | "self-update"
            | "setup"
            | "store"
            | "with",
    )
}

fn package_manager_switch_disabled() -> bool {
    PACKAGE_MANAGER_SWITCH_ENV_VARS.into_iter().any(env_var_is_false)
}

fn env_var_is_false(name: &str) -> bool {
    std::env::var_os(name)
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "false" | "0"))
}

#[derive(Clone, Copy)]
struct SwitchProcessState {
    package_manager_switch_disabled: bool,
    executed_by_corepack: bool,
}

impl SwitchProcessState {
    fn current() -> Self {
        Self {
            package_manager_switch_disabled: package_manager_switch_disabled(),
            executed_by_corepack: std::env::var_os("COREPACK_ROOT").is_some(),
        }
    }
}

#[derive(Debug)]
struct SwitchTarget {
    spec: String,
    source: SwitchSource,
}

#[derive(Debug)]
pub(crate) struct SwitchPlan {
    config: Config,
    target: SwitchTarget,
}

#[derive(Debug)]
enum SwitchSource {
    LockedEnv { env: EnvLockfile, version: String },
    Resolve { env_root: PathBuf },
}

struct SwitchInput {
    dir: PathBuf,
    npmrc_auth_file: Option<PathBuf>,
    command: Option<String>,
}

impl SwitchInput {
    fn from_cli_args(args: &CliArgs) -> Self {
        Self {
            dir: args.dir.clone(),
            npmrc_auth_file: args.npmrc_auth_file.clone(),
            command: Some(command_name(&args.command).to_string()),
        }
    }

    fn from_version_argv(argv: &[OsString]) -> Self {
        let mut input = Self { dir: PathBuf::from("."), npmrc_auth_file: None, command: None };
        let mut index = 1;
        while index < argv.len() {
            let Some(token) = argv[index].to_str() else {
                input.command = Some(String::new());
                break;
            };
            if token == "--" {
                break;
            }
            if !token.starts_with('-') {
                input.command = Some(token.to_string());
                break;
            }
            if let Some(value) =
                short_value(token, "-C", argv.get(index + 1).map(OsString::as_os_str))
            {
                input.dir = PathBuf::from(value);
                index += if token == "-C" { 2 } else { 1 };
                continue;
            }
            if let Some((value, width)) =
                long_value(token, "dir", argv.get(index + 1).map(OsString::as_os_str))
            {
                input.dir = PathBuf::from(value);
                index += width;
                continue;
            }
            if let Some((value, width)) =
                long_value(token, "npmrc-auth-file", argv.get(index + 1).map(OsString::as_os_str))
                    .or_else(|| {
                        long_value(
                            token,
                            "userconfig",
                            argv.get(index + 1).map(OsString::as_os_str),
                        )
                    })
            {
                input.npmrc_auth_file = Some(PathBuf::from(value));
                index += width;
                continue;
            }
            index += if value_taking_global_option(token) { 2 } else { 1 };
        }
        input
    }
}

fn command_name(command: &CliCommand) -> &'static str {
    match command {
        CliCommand::Access(_) => "access",
        CliCommand::Init => "init",
        CliCommand::Recursive => "recursive",
        CliCommand::Add(_) => "add",
        CliCommand::Install(_) => "install",
        CliCommand::InstallTest(_) => "install-test",
        CliCommand::Update(_) => "update",
        CliCommand::Outdated(_) => "outdated",
        CliCommand::Audit(_) => "audit",
        CliCommand::Change(_) => "change",
        CliCommand::Version(_) => "version",
        CliCommand::Lane(_) => "lane",
        CliCommand::Bugs(_) => "bugs",
        CliCommand::List(_) => "list",
        CliCommand::Ll(_) => "ll",
        CliCommand::Licenses(_) => "licenses",
        CliCommand::Why(_) => "why",
        CliCommand::View(_) => "view",
        CliCommand::Sbom(_) => "sbom",
        CliCommand::Whoami => "whoami",
        CliCommand::Deprecate(_) => "deprecate",
        CliCommand::Undeprecate(_) => "undeprecate",
        CliCommand::Unpublish(_) => "unpublish",
        CliCommand::Star(_) => "star",
        CliCommand::Unstar(_) => "unstar",
        CliCommand::Stars(_) => "stars",
        CliCommand::DistTag(_) => "dist-tag",
        CliCommand::Ping(_) => "ping",
        CliCommand::Doctor(_) => "doctor",
        CliCommand::Search(_) => "search",
        CliCommand::Rebuild(_) => "rebuild",
        CliCommand::Pack(_) => "pack",
        CliCommand::Publish(_) => "publish",
        CliCommand::Stage(_) => "stage",
        CliCommand::Remove(_) => "remove",
        CliCommand::Patch(_) => "patch",
        CliCommand::PatchCommit(_) => "patch-commit",
        CliCommand::PatchRemove(_) => "patch-remove",
        CliCommand::Peers(_) => "peers",
        CliCommand::SetScript(_) => "set-script",
        CliCommand::Test => "test",
        CliCommand::Run(_) => "run",
        CliCommand::External(_) => "external",
        CliCommand::Exec(_) => "exec",
        CliCommand::Dlx(_) => "dlx",
        CliCommand::Create(_) => "create",
        CliCommand::Start => "start",
        CliCommand::Stop(_) => "stop",
        CliCommand::Restart(_) => "restart",
        CliCommand::FindHash(_) => "find-hash",
        CliCommand::Runtime(_) => "runtime",
        CliCommand::Bin(_) => "bin",
        CliCommand::Clean(_) => "clean",
        CliCommand::Purge(_) => "purge",
        CliCommand::Ci(_) => "ci",
        CliCommand::Root(_) => "root",
        CliCommand::Prefix(_) => "prefix",
        CliCommand::Config(_) => "config",
        CliCommand::Pkg(_) => "pkg",
        CliCommand::PackApp(_) => "pack-app",
        CliCommand::Store(_) => "store",
        CliCommand::Cache(_) => "cache",
        CliCommand::CatFile(_) => "cat-file",
        CliCommand::CatIndex(_) => "cat-index",
        CliCommand::IgnoredBuilds(_) => "ignored-builds",
        CliCommand::ApproveBuilds(_) => "approve-builds",
        CliCommand::Link(_) => "link",
        CliCommand::Import(_) => "import",
        CliCommand::Dedupe(_) => "dedupe",
        CliCommand::Deploy(_) => "deploy",
        CliCommand::Prune(_) => "prune",
        CliCommand::Fetch(_) => "fetch",
        CliCommand::Unlink(_) => "unlink",
        CliCommand::Docs(_) => "docs",
        CliCommand::Repo(_) => "repo",
        CliCommand::SelfUpdate(_) => "self-update",
        CliCommand::Setup(_) => "setup",
        CliCommand::Login(_) => "login",
        CliCommand::Logout(_) => "logout",
        CliCommand::With(_) => "with",
        CliCommand::Completion(_) => "completion",
        CliCommand::CompletionServer(_) => "completion-server",
        CliCommand::Team(_) => "team",
        CliCommand::Owner(_) => "owner",
    }
}

fn short_value<'a>(token: &'a str, option: &str, next: Option<&'a OsStr>) -> Option<&'a OsStr> {
    if token == option {
        return next;
    }
    token.strip_prefix(option).filter(|value| !value.is_empty()).map(OsStr::new)
}

fn long_value<'a>(
    token: &'a str,
    option: &str,
    next: Option<&'a OsStr>,
) -> Option<(&'a OsStr, usize)> {
    let name = token.strip_prefix("--")?;
    if name == option {
        return next.map(|value| (value, 2));
    }
    name.strip_prefix(option)
        .and_then(|rest| rest.strip_prefix('='))
        .map(OsStr::new)
        .map(|value| (value, 1))
}

fn value_taking_global_option(token: &str) -> bool {
    if matches!(token, "-F") {
        return true;
    }
    if token.starts_with("-F") {
        return false;
    }
    let Some(name) = token.strip_prefix("--") else {
        return false;
    };
    if name.contains('=') {
        return false;
    }
    matches!(name, "filter" | "filter-prod" | "npmrc-auth-file" | "reporter" | "userconfig")
}

#[cfg(test)]
mod tests;
