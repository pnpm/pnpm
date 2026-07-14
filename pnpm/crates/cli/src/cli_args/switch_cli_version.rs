use super::{
    cli_command::{CliArgs, CliCommand},
    package_manager::{
        PACKAGE_MANAGER_SWITCH_ENV_VARS, WantedPackageManager, read_manifest_json,
        should_persist_package_manager_lockfile, version_satisfies, wanted_package_manager,
    },
    self_update::install_pnpm::pnpm_package_to_install,
    with::{
        PackageManagerCheck,
        install_pnpm_to_store::{install_pnpm_from_env, install_pnpm_to_store},
        spawn_pnpm,
    },
};
use crate::{config_deps, config_overrides::ConfigOverrides};
use miette::{Context, IntoDiagnostic};
use pacquet_config::{Config, Host, PNPM_VERSION, PmOnFail};
use pacquet_lockfile::{EnvLockfile, LockfileResolution, PackageKey, PackageMetadata, VersionPart};
use pacquet_reporter::SilentReporter;
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    fmt::Display,
    path::{Path, PathBuf},
};

pub(crate) fn switch_plan(
    args: &CliArgs,
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<SwitchPlan>> {
    if should_skip_command(&args.command) {
        return Ok(None);
    }
    switch_plan_from_input(
        &SwitchInput::from_cli_args(args),
        config_overrides,
        SwitchProcessState::current(),
    )
}

pub(crate) fn switch_plan_for_version_flag(
    argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<SwitchPlan>> {
    switch_plan_from_input(
        &SwitchInput::from_version_argv(argv),
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

fn switch_plan_from_input(
    input: &SwitchInput,
    config_overrides: &ConfigOverrides,
    process_state: SwitchProcessState,
) -> miette::Result<Option<SwitchPlan>> {
    if input.command.as_deref().is_some_and(should_skip_command_name)
        || process_state.package_manager_switch_disabled
        || process_state.executed_by_corepack
    {
        return Ok(None);
    }
    let dir = dunce::canonicalize(&input.dir).into_diagnostic().wrap_err_with(|| {
        format!("canonicalizing the `--dir` argument: {}", input.dir.display())
    })?;
    let mut config = Config { npmrc_auth_file: input.npmrc_auth_file.clone(), ..Config::default() }
        .current::<Host>(&dir)
        .map_err(miette::Report::new)
        .wrap_err("load configuration")?;
    config_overrides.apply(&mut config);

    let root_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.clone());
    let Some(target) = switch_target(&config, &root_dir)? else {
        return Ok(None);
    };
    if version_satisfies(PNPM_VERSION, &target.spec) {
        return Ok(None);
    }
    Ok(Some(SwitchPlan { config, target }))
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

fn invalid_package_manager_lockfile(dep_path: impl Display) -> miette::Report {
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

fn should_skip_command(command: &CliCommand) -> bool {
    matches!(
        command,
        CliCommand::Completion(_)
            | CliCommand::CompletionServer(_)
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
        "completion"
            | "completion-server"
            | "env"
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
        CliCommand::Add(_) => "add",
        CliCommand::Install(_) => "install",
        CliCommand::Update(_) => "update",
        CliCommand::Outdated(_) => "outdated",
        CliCommand::Audit(_) => "audit",
        CliCommand::Change(_) => "change",
        CliCommand::Version(_) => "version",
        CliCommand::Lane(_) => "lane",
        CliCommand::Bugs(_) => "bugs",
        CliCommand::List(_) => "list",
        CliCommand::Ll(_) => "ll",
        CliCommand::Why(_) => "why",
        CliCommand::Sbom(_) => "sbom",
        CliCommand::Whoami => "whoami",
        CliCommand::Deprecate(_) => "deprecate",
        CliCommand::Undeprecate(_) => "undeprecate",
        CliCommand::Star(_) => "star",
        CliCommand::Unstar(_) => "unstar",
        CliCommand::Stars(_) => "stars",
        CliCommand::DistTag(_) => "dist-tag",
        CliCommand::Ping(_) => "ping",
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
