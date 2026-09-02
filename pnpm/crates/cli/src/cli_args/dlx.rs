use crate::{
    State,
    cli_args::{
        add::add_package, catalogs::configured_catalogs,
        supported_architectures::SupportedArchitecturesArgs,
    },
    engine_pm::{channel::PackageManager, provision::provision},
    path_env::{BadPathDir, prepend_dirs_to_path, set_command_path},
    shim_dispatch::materialize_runtime,
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_resolver::{
    CatalogResolutionResult, WantedDependency as CatalogWantedDependency, resolve_from_catalog,
};
use pnpm_cmd_shim::{Host as CmdShimHost, get_bins_from_package_manifest};
use pnpm_config::Config;
use pnpm_config_parse_overrides::parse_overrides_iter;
use pnpm_crypto_hash::create_short_hash;
use pnpm_fs::force_symlink_dir;
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_package_manifest::{
    DependencyGroup, convert_engines_runtime_to_dependencies, is_runtime_alias,
    package_manager_spec::{is_version_request, split_spec},
    parse_manifest,
};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Run a package in a temporary environment.
///
/// A command naming a package manager (`yarn`, `npm`, `bun`, `pnpm`) or a
/// runtime (`node`, `deno`, `bun`) is provisioned rather than installed
/// from the package that shares its name: those npm packages are either a
/// different line of the tool (`yarn` stops at Classic) or a wrapper that
/// downloads it.
#[derive(Debug, Args)]
pub struct DlxArgs {
    /// The command to run, followed by its arguments.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,

    /// The package to install before running the command. May be
    /// repeated. When omitted, the command name is the package.
    #[clap(long = "package")]
    pub package: Vec<String>,

    /// Package names allowed to run lifecycle (build) scripts during
    /// the dlx install. May be repeated.
    #[clap(long = "allow-build")]
    pub allow_build: Vec<String>,

    /// Run the command inside of a shell. Uses `/bin/sh` on UNIX and
    /// `cmd.exe` on Windows.
    #[clap(long, short = 'c')]
    pub shell_mode: bool,

    // The architecture overrides take a single comma-separable value per
    // occurrence (`--cpu arm64,x64`) rather than the greedy `num_args =
    // 1..` shape `SupportedArchitecturesArgs` uses for `install` / `add`:
    // dlx's trailing `command` positional would otherwise be swallowed as
    // extra `--cpu` values. They override the per-axis
    // `supportedArchitectures` of the dlx install only.
    /// CPU architectures whose platform-tagged optional dependencies the
    /// dlx install should keep. Repeat or comma-separate for multiple.
    #[clap(long, value_delimiter = ',')]
    pub cpu: Vec<String>,

    /// Operating systems whose platform-tagged optional dependencies the
    /// dlx install should keep.
    #[clap(long, value_delimiter = ',')]
    pub os: Vec<String>,

    /// libc families (`glibc`, `musl`) whose platform-tagged optional
    /// dependencies the dlx install should keep.
    #[clap(long, value_delimiter = ',')]
    pub libc: Vec<String>,
}

/// Errors from `pacquet dlx`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum DlxError {
    #[display("'pnpm dlx' requires a command to run")]
    #[diagnostic(code(ERR_PNPM_DLX_MISSING_COMMAND))]
    MissingCommand,

    #[display(r#"dlx was unable to find the installed dependency in "dependencies""#)]
    #[diagnostic(code(ERR_PNPM_DLX_NO_DEP))]
    NoDep,

    #[display("No binaries found in {package}")]
    #[diagnostic(code(ERR_PNPM_DLX_NO_BIN))]
    NoBin { package: String },

    #[display("Could not determine executable to run. {package} has multiple binaries: {bins}")]
    #[diagnostic(
        code(ERR_PNPM_DLX_MULTIPLE_BINS),
        help("Pass --package=<name> and choose one of: {bins}")
    )]
    MultipleBins { package: String, bins: String },

    #[display("Command \"{command}\" not found")]
    #[diagnostic(code(ERR_PNPM_DLX_COMMAND_NOT_FOUND))]
    CommandNotFound { command: String },

    #[display(
        "Cannot add {dir} to PATH because it contains the path delimiter character ({delimiter})"
    )]
    #[diagnostic(code(ERR_PNPM_BAD_PATH_DIR))]
    BadPathDir { dir: String, delimiter: char },

    #[display("Failed to read the installed manifest at {path}: {source}")]
    #[diagnostic(code(ERR_PNPM_CLI_DLX_READ_MANIFEST))]
    ReadManifest {
        path: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to prepare the dlx cache directory {dir}: {source}")]
    #[diagnostic(code(ERR_PNPM_CLI_DLX_CACHE))]
    Cache {
        dir: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to spawn command \"{command}\": {source}")]
    #[diagnostic(code(ERR_PNPM_CLI_DLX_SPAWN))]
    Spawn {
        command: String,
        #[error(source)]
        source: std::io::Error,
    },
}

impl From<BadPathDir> for DlxError {
    fn from(BadPathDir { dir, delimiter }: BadPathDir) -> Self {
        DlxError::BadPathDir { dir, delimiter }
    }
}

impl DlxArgs {
    /// Execute the subcommand. The package is installed into a cache
    /// directory under `config.cache_dir`, and the resolved bin runs in
    /// the process working directory (`cwd: process.cwd()`). `dir` is only
    /// the fallback when the process cwd can't be read.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        config: &'static mut Config,
    ) -> miette::Result<()> {
        let DlxArgs { command, package, allow_build, shell_mode, cpu, os, libc } = self;
        let supported_architectures = SupportedArchitecturesArgs { cpu, os, libc };
        let Some((bin_command, args)) = command.split_first() else {
            return Err(DlxError::MissingCommand.into());
        };

        // Read the config values every dlx spawn applies, before the
        // install path below consumes `config` to anchor it at the cache
        // directory.
        let extra_bin_paths = config.extra_bin_paths.clone();
        let mut extra_env = config.extra_env.clone();
        // The GVS resolution env injected by `Config::current` points at
        // the *invoking* project's node_modules; the dlx tool runs from
        // its own self-contained cache (GVS forced off for the cache
        // install), so inheriting it would let the tool resolve phantom
        // deps from the caller's tree.
        extra_env.remove("NODE_PATH");
        extra_env.remove("NODE_OPTIONS");
        let user_agent = config.user_agent.clone();

        // The dlx command runs in the process working directory
        // (`cwd: process.cwd()`), independent of `--dir`.
        let run_cwd = std::env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
        let spawn = DlxSpawn {
            cwd: &run_cwd,
            extra_bin_paths: &extra_bin_paths,
            extra_env: &extra_env,
            user_agent: &user_agent,
            shell_mode,
        };

        // An explicit `--package` is the user naming what to install, so
        // it stays literal; a bare tool name is provisioned instead of
        // fetched (see `engine_pm::selector` for why the name alone is
        // not enough to install one).
        if package.is_empty()
            && let Some((pm, version_spec)) = parse_package_manager_spec(bin_command)
        {
            return run_package_manager::<Reporter>(
                config,
                pm,
                version_spec,
                bin_command,
                None,
                args,
                &spawn,
            )
            .await;
        }

        // A package manager publishes more than one command, so naming it
        // with `--package` says which engine to provision while the
        // command says which of its bins to run: `pnx --package npm@11 npx`.
        if let [spec] = package.as_slice()
            && let Some((pm, version_spec)) = parse_package_manager_spec(spec)
            && pm.bins().contains(&bin_command.as_str())
        {
            return run_package_manager::<Reporter>(
                config,
                pm,
                version_spec,
                spec,
                Some(bin_command),
                args,
                &spawn,
            )
            .await;
        }

        if package.is_empty()
            && let Some((name, version_spec)) = parse_runtime_spec(bin_command)
        {
            return run_runtime(&config.state_dir, name, version_spec, bin_command, args, &spawn)
                .await;
        }

        // `pkgs = package ?? [command]`. With `--package`, the command
        // names the bin to run; otherwise the command is also the package.
        let pkgs: Vec<String> =
            if package.is_empty() { vec![bin_command.clone()] } else { package.clone() };
        // Resolved here rather than in the install below so the catalog's
        // version also feeds the cache key: two callers whose catalogs pin
        // different versions of the same package must not share a cache
        // entry.
        let pkgs = resolve_catalog_specs(&pkgs, config)?;

        // Read the config values needed before (and after) the install,
        // because the install path consumes `config` to anchor it at the
        // cache directory.
        let cache_dir = config.cache_dir.clone();
        let max_age = config.dlx_cache_max_age;
        let registries = build_registries_map(config);
        // The effective (post-`--cpu`/`--os`/`--libc`) architecture set
        // is part of the cache key: it changes which platform-tagged
        // optional dependencies get installed, so two invocations that
        // differ only by architecture must not share a cache entry.
        // `supportedArchitectures` is fed into the cache key for this.
        let effective_architectures =
            supported_architectures.apply_to(config.supported_architectures.clone());
        let cache_key =
            create_cache_key(&pkgs, &registries, &allow_build, effective_architectures.as_ref());
        let dlx_command_cache_dir = cache_dir.join("dlx").join(&cache_key);
        fs::create_dir_all(&dlx_command_cache_dir).map_err(|source| DlxError::Cache {
            dir: dlx_command_cache_dir.display().to_string(),
            source,
        })?;
        // Canonicalize so the prepare dir carries no `..` segments. A
        // relative `cacheDir` (e.g. `../pnpm-cache`) would otherwise
        // let the install's workspace-root walk pass through the
        // caller's project dir and mistake it for the dlx workspace.
        let dlx_command_cache_dir = dunce::canonicalize(&dlx_command_cache_dir)
            .into_diagnostic()
            .wrap_err("canonicalizing the dlx cache directory")?;
        let cache_link = dlx_command_cache_dir.join("pkg");

        let cached_dir =
            if let Some(dir) = get_valid_cache_dir(&cache_link, max_age, SystemTime::now()) {
                dir
            } else {
                let prepare_dir =
                    get_prepare_dir(&dlx_command_cache_dir, SystemTime::now(), std::process::id());
                if let Err(error) = install_into_cache::<Reporter>(
                    &prepare_dir,
                    &pkgs,
                    &allow_build,
                    &supported_architectures,
                    config,
                )
                .await
                {
                    // Don't leave a half-installed prepare dir behind to
                    // accumulate across failed runs: remove it on install
                    // failure. Best-effort cleanup.
                    let _ = fs::remove_dir_all(&prepare_dir);
                    return Err(error);
                }
                // Best-effort: a parallel dlx process may have raced
                // us to the link. Either link is equally fresh, so
                // ignore the failure and run from our own prepare dir.
                let _ = force_symlink_dir(&prepare_dir, &cache_link);
                prepare_dir
            };

        let bins_dir = cached_dir.join("node_modules").join(".bin");
        let bin_name =
            if package.is_empty() { get_bin_name(&cached_dir)? } else { bin_command.clone() };

        run_bin(DlxProgram::Named(&bin_name), args, vec![bins_dir], &spawn)
    }
}

/// Install `pkgs` into `prepare_dir` so their bins land in
/// `<prepare_dir>/node_modules/.bin`. Anchors `config` at the cache
/// directory (the `dir` / `lockfileDir` / `bin` overrides) and saves each
/// package to `dependencies`.
async fn install_into_cache<Reporter: self::Reporter + 'static>(
    prepare_dir: &Path,
    pkgs: &[String],
    allow_build: &[String],
    supported_architectures: &SupportedArchitecturesArgs,
    config: &'static mut Config,
) -> miette::Result<()> {
    fs::create_dir_all(prepare_dir)
        .map_err(|source| DlxError::Cache { dir: prepare_dir.display().to_string(), source })?;
    let manifest_path = prepare_dir.join("package.json");
    fs::write(&manifest_path, json!({ "name": "dlx", "version": "0.0.0" }).to_string())
        .map_err(|source| DlxError::Cache { dir: manifest_path.display().to_string(), source })?;

    // Per-axis CLI overrides (`--cpu` / `--os` / `--libc`) replace the
    // matching axis of the config-derived value for the dlx install.
    config.supported_architectures =
        supported_architectures.apply_to(config.supported_architectures.clone());

    config.modules_dir = prepare_dir.join("node_modules");
    config.virtual_store_dir = prepare_dir.join("node_modules").join(".pacquet");
    // Force the project-local virtual store so the whole prepare dir is
    // self-contained and can be symlinked as the cache entry. This is a
    // deliberate deviation from pnpm's dlx, which keeps
    // `enableGlobalVirtualStore ?? true`: pnpm caches only the
    // `node_modules` tree and lets the global store back it, whereas
    // pacquet symlinks the entire prepare dir, so its store must live
    // inside that dir (the installer picks `global_virtual_store_dir`
    // when this is on — see virtual_store_layout.rs).
    config.enable_global_virtual_store = false;
    // The cache install is always fresh, so no lockfile is loaded from
    // the process working directory.
    config.lockfile = false;
    // The cache install inherits the caller project's `overrides` (pnpm's
    // dlx likewise runs its install with the invoking project's
    // already-loaded config), and a `catalog:` value in them resolves
    // against the caller's catalogs. Those catalogs go with `workspace_dir`,
    // which is severed right below — resolve the references now, or the
    // install would look them up against an empty catalog set and fail with
    // ERR_PNPM_CATALOG_IN_OVERRIDES.
    if let Some(overrides) = config.overrides.as_ref()
        && config.workspace_dir.is_some()
        && overrides.values().any(|spec| spec.starts_with("catalog:"))
    {
        let catalogs = configured_catalogs(config)?;
        let resolved = parse_overrides_iter(overrides.iter(), &catalogs)
            .map_err(miette::Report::new)?
            .into_iter()
            .map(|entry| (entry.selector, entry.new_bare_specifier))
            .collect();
        config.overrides = Some(resolved);
    }
    // The throwaway cache project is not part of the caller's
    // workspace. If a caller has a settings-only pnpm-workspace.yaml,
    // carrying its workspace root here makes the install enumerate that
    // workspace and fail on the missing root package.json. Anchored
    // rather than `None`, which walks up from the cache dir and can
    // adopt a stray `pnpm-workspace.yaml` above it (pnpm/pnpm#13697).
    config.workspace_dir = Some(prepare_dir.to_path_buf());
    // Same reasoning for a pinned `lockfileDir`: it names the caller's
    // lockfile, which the throwaway install must not touch.
    config.lockfile_dir = None;
    // The caller's patches never apply to the throwaway install (pnpm's
    // dlx installs the package unpatched too). Their paths are relative
    // to the caller's workspace root, which the anchor above replaced, so
    // keeping them would also make every dlx invocation from a project
    // with `patchedDependencies` fail on a patch file missing under the
    // cache dir.
    config.patched_dependencies = None;
    // Build a *fresh* allow-list for the throwaway install — the dlx
    // packages themselves plus the CLI `--allow-build` entries — rather
    // than inheriting the caller project's `allow_builds` /
    // `dangerously_allow_all_builds`. Inheriting the caller's policy would
    // run build scripts the dlx invocation never opted into, and would
    // also leave the cache key (which hashes only pkgs + CLI allow_build)
    // unable to distinguish two callers with different policies.
    config.dangerously_allow_all_builds = false;
    config.allow_builds.clear();
    for spec in pkgs {
        if let Some(alias) = parse_wanted_dependency(spec).alias {
            config.allow_builds.insert(alias, true);
        }
    }
    for name in allow_build {
        config.allow_builds.insert(name.clone(), true);
    }
    let config: &Config = config;

    for pkg in pkgs {
        let state = State::init(manifest_path.clone(), config, false)
            .wrap_err("initialize the dlx install state")?;
        add_package::<Reporter, _>(
            state,
            pkg,
            // dlx records the default caret range; the spec is throwaway.
            RangeSpecStyle::default(),
            // dlx never catalogs.
            None,
            // dlx must download to run the bin, so never lockfile-only.
            false,
            config.supported_architectures.clone(),
            [DependencyGroup::Prod],
        )
        .await?;
    }
    Ok(())
}

/// How dlx spawns whatever it ends up running: the working directory and
/// shell mode the command was invoked with, plus the environment read off
/// `Config` before the install path consumes it.
struct DlxSpawn<'a> {
    cwd: &'a Path,
    extra_bin_paths: &'a [PathBuf],
    extra_env: &'a HashMap<String, String>,
    user_agent: &'a str,
    shell_mode: bool,
}

/// What dlx runs.
#[derive(Clone, Copy)]
enum DlxProgram<'a> {
    /// A bin from the installed package, looked up on the prepared `PATH`.
    Named(&'a str),
    /// A provisioned engine's own executable, run by path rather than by
    /// name: an engine's directory can hold another executable named the
    /// way a user would type it — Yarn 6's archive ships a `yarn` launcher
    /// beside the `yarn-bin` that is the engine itself.
    Provisioned { command: &'a str, executable: &'a Path },
}

impl DlxProgram<'_> {
    /// The word a shell runs the command by — both forms are reachable
    /// through the prepended directories — or `None` when there is no
    /// word a shell could carry, which only a provisioned executable
    /// whose file name is not valid UTF-8 can produce.
    fn shell_word(&self) -> Option<&str> {
        match self {
            DlxProgram::Named(name) => Some(name),
            DlxProgram::Provisioned { executable, .. } => {
                executable.file_name().and_then(std::ffi::OsStr::to_str)
            }
        }
    }

    fn command(&self) -> &str {
        match self {
            DlxProgram::Named(name) => name,
            DlxProgram::Provisioned { command, .. } => command,
        }
    }
}

fn run_bin(
    program: DlxProgram<'_>,
    args: &[String],
    bin_dirs: Vec<PathBuf>,
    spawn: &DlxSpawn<'_>,
) -> miette::Result<()> {
    let DlxSpawn { cwd, extra_bin_paths, extra_env, user_agent, shell_mode } = *spawn;
    let mut prepend = bin_dirs;
    prepend.extend(extra_bin_paths.iter().cloned());
    let path = prepend_dirs_to_path(&prepend).map_err(DlxError::from)?;

    let mut cmd = if shell_mode {
        let shell = pnpm_executor::select_shell(None, cfg!(windows))
            .expect("default shell selection never fails");
        let word = program
            .shell_word()
            .ok_or_else(|| DlxError::CommandNotFound { command: program.command().to_string() })?;
        let mut joined = vec![word.to_string()];
        joined.extend(args.iter().cloned());
        let mut cmd = Command::new(&shell.program);
        cmd.args(&shell.args);
        // Append the joined command through `push_script_arg` so the
        // Windows `cmd /d /s /c` verbatim path uses `raw_arg`, matching
        // execa's `windowsVerbatimArguments` and preserving embedded
        // quoting (same as exec's shell mode).
        pnpm_executor::push_script_arg(&mut cmd, &joined.join(" "), shell.windows_verbatim_args);
        cmd
    } else {
        let executable = match program {
            DlxProgram::Named(name) => which::which_in(name, Some(&path), cwd)
                .map_err(|_| DlxError::CommandNotFound { command: name.to_string() })?,
            DlxProgram::Provisioned { executable, .. } => executable.to_path_buf(),
        };
        let mut cmd = Command::new(executable);
        cmd.args(args);
        cmd
    };

    cmd.current_dir(cwd);
    // `updateConfig`-provided env, applied first so pnpm's own keys win
    // on conflict (matching `exec`'s spawn and TS `makeEnv`). dlx does
    // not run the `updateConfig` hook, so this is currently always
    // empty; wired for uniformity with the other spawn sites and so it
    // works if that changes.
    cmd.envs(extra_env);
    set_command_path(&mut cmd, &path);
    cmd.env("npm_config_user_agent", user_agent);

    let status = pnpm_executor::spawn_child(&mut cmd, None)
        .and_then(|mut child| child.wait())
        .map_err(|source| DlxError::Spawn { command: program.command().to_string(), source })?;
    if !status.success() {
        #[expect(
            clippy::exit,
            reason = "dlx propagates the spawned command's exit status, like pnpm"
        )]
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Split a dlx command word into the package manager it names and the
/// version specifier it asks for, or `None` when it names something else.
///
/// The bare name is the package manager's own line — `pnx yarn` is
/// whatever Yarn currently ships — matching how a `dlx` package spec
/// without a version resolves.
fn parse_package_manager_spec(command: &str) -> Option<(PackageManager, &str)> {
    let (name, version_spec) = split_spec(command);
    let version_spec = version_spec.unwrap_or("latest");
    // A specifier that locates a package names what to install, not a line
    // of the tool, so it stays on the ordinary dlx path.
    if !is_version_request(version_spec) {
        return None;
    }
    Some((PackageManager::parse(name)?, version_spec))
}

/// Split a dlx command word into the runtime it names and the version
/// specifier it asks for, or `None` when it names something else.
fn parse_runtime_spec(command: &str) -> Option<(&str, &str)> {
    let (name, version_spec) = split_spec(command);
    let version_spec = version_spec.unwrap_or("latest");
    // `node@runtime:22` is the same request spelled with the protocol the
    // resolver uses; anything else that locates a package is an ordinary
    // one.
    let version_spec = match version_spec.strip_prefix("runtime:") {
        Some(version_spec) => version_spec,
        None if is_version_request(version_spec) => version_spec,
        None => return None,
    };
    is_runtime_alias(name).then_some((name, version_spec))
}

/// Materialize `name` at `version_spec` and run it, the runtime half of
/// [`run_package_manager`].
async fn run_runtime(
    state_dir: &Path,
    name: &str,
    version_spec: &str,
    command: &str,
    args: &[String],
    spawn: &DlxSpawn<'_>,
) -> miette::Result<()> {
    let executable =
        Box::pin(materialize_runtime(state_dir, name.to_string(), version_spec.to_string()))
            .await?;
    let bin_dirs: Vec<PathBuf> = executable.parent().map(Path::to_path_buf).into_iter().collect();
    run_bin(DlxProgram::Provisioned { command, executable: &executable }, args, bin_dirs, spawn)
}

/// Provision `pm` and run `bin` — or the engine's own command, when the
/// caller named none — with the caller's arguments, propagating the exit
/// status the way the rest of dlx does.
async fn run_package_manager<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    version_spec: &str,
    command: &str,
    bin: Option<&str>,
    args: &[String],
    spawn: &DlxSpawn<'_>,
) -> miette::Result<()> {
    let engine = Box::pin(provision::<Reporter>(config, pm, version_spec)).await?;
    let executable = match bin {
        Some(bin) => engine.command(bin),
        None => engine.program.clone(),
    };
    run_bin(
        DlxProgram::Provisioned { command, executable: &executable },
        args,
        engine.bin_dirs,
        spawn,
    )
}

/// Replace the `catalog:` specifier of each dlx package spec with the
/// specifier the caller's catalogs hold. Any other spec passes through
/// untouched, and the catalogs are only read when at least one spec needs
/// them. A misconfigured entry is reported as the pnpm error the caller
/// would get from `pnpm add`.
fn resolve_catalog_specs(pkgs: &[String], config: &Config) -> miette::Result<Vec<String>> {
    let uses_catalog = |pkg: &String| {
        parse_wanted_dependency(pkg)
            .bare_specifier
            .is_some_and(|bare_specifier| parse_catalog_protocol(&bare_specifier).is_some())
    };
    if !pkgs.iter().any(uses_catalog) {
        return Ok(pkgs.to_vec());
    }
    let catalogs = configured_catalogs(config)?;
    pkgs.iter()
        .map(|pkg| {
            let parsed = parse_wanted_dependency(pkg);
            let (Some(alias), Some(bare_specifier)) = (parsed.alias, parsed.bare_specifier) else {
                return Ok(pkg.clone());
            };
            let wanted = CatalogWantedDependency { alias: alias.clone(), bare_specifier };
            match resolve_from_catalog(&catalogs, &wanted) {
                CatalogResolutionResult::Found(found) => {
                    Ok(format!("{alias}@{}", found.resolution.specifier))
                }
                CatalogResolutionResult::Unused => Ok(pkg.clone()),
                CatalogResolutionResult::Misconfiguration(misconfiguration) => {
                    Err(miette::Report::new(misconfiguration.error))
                }
            }
        })
        .collect()
}

/// Build the `{ "default": registry, <alias>: url, … }` map fed into the
/// cache key.
fn build_registries_map(config: &Config) -> BTreeMap<String, String> {
    let mut map = config.resolved_registries();
    for (name, url) in &config.registries_by_prefix {
        map.insert(name.clone(), url.clone());
    }
    map
}

/// Build the dlx cache key from the sorted package specs, sorted
/// registries, the optional `allow_build` list, and each non-empty
/// `supportedArchitectures` axis (deduped + sorted, in `cpu` / `libc` /
/// `os` order), all hashed together. pacquet keys on the raw specs (not
/// resolved ids) and uses [`create_short_hash`] rather than a full-length
/// hex digest; the dlx caches are not shared between the two
/// implementations, so the key format is not a cross-tool contract.
fn create_cache_key(
    pkgs: &[String],
    registries: &BTreeMap<String, String>,
    allow_build: &[String],
    supported_architectures: Option<&SupportedArchitectures>,
) -> String {
    let mut sorted: Vec<&str> = pkgs.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let registry_pairs: Vec<(&str, &str)> =
        registries.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut args = vec![json!(sorted), json!(registry_pairs)];
    if !allow_build.is_empty() {
        let mut sorted_allow: Vec<&str> = allow_build.iter().map(String::as_str).collect();
        sorted_allow.sort_unstable();
        args.push(json!({ "allowBuild": sorted_allow }));
    }
    if let Some(arch) = supported_architectures {
        for (key, values) in [("cpu", &arch.cpu), ("libc", &arch.libc), ("os", &arch.os)] {
            let Some(values) = values.as_ref().filter(|values| !values.is_empty()) else {
                continue;
            };
            let mut deduped: Vec<&str> = values.iter().map(String::as_str).collect();
            deduped.sort_unstable();
            deduped.dedup();
            args.push(json!({ "supportedArchitectures": { key: deduped } }));
        }
    }
    create_short_hash(&serde_json::to_string(&args).expect("serialize cache key inputs"))
}

/// Return the cache target behind `cache_link` when it is a symlink whose
/// own mtime is within `max_age_minutes` of `now`.
fn get_valid_cache_dir(
    cache_link: &Path,
    max_age_minutes: u64,
    now: SystemTime,
) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(cache_link).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    // `dunce::canonicalize` (not `fs::canonicalize`) so the cache-hit path
    // matches the fresh-install branch's form — on Windows `fs::canonicalize`
    // returns a `\\?\` verbatim path that would feed a different
    // `node_modules/.bin` string into `PATH`.
    let target = dunce::canonicalize(cache_link).ok()?;
    let mtime = meta.modified().ok()?;
    let max_age = Duration::from_secs(max_age_minutes.saturating_mul(60));
    // Valid while `mtime + max_age >= now`. A negative elapsed time
    // (clock skew, `now` before `mtime`) is treated as still valid,
    // matching pnpm's numeric comparison.
    match now.duration_since(mtime) {
        Ok(age) => (age <= max_age).then_some(target),
        Err(_) => Some(target),
    }
}

/// The timestamped, pid-scoped subdirectory a fresh dlx install is
/// prepared in.
fn get_prepare_dir(cache_path: &Path, now: SystemTime, pid: u32) -> PathBuf {
    let millis = now.duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis());
    // base36 (vs hex) keeps this segment short: it sits between the cache key
    // and pnpm's deep virtual-store layout, and long dlx paths overflow
    // Windows' MAX_PATH (260), which makes lifecycle scripts fail with a
    // `spawn cmd.exe ENOENT` (the cwd no longer resolves). time+pid stays
    // unique across concurrent dlx processes and a process's own retries.
    cache_path.join(format!("{}-{}", to_base36(millis), to_base36(u128::from(pid))))
}

/// Lowercase base36 (`0-9a-z`), matching JavaScript's
/// `Number.prototype.toString(36)` used by `getPrepareDir`.
fn to_base36(mut n: u128) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).expect("base36 digits are ASCII")
}

/// Determine the bin to run from the first installed dependency.
fn get_bin_name(cached_dir: &Path) -> Result<String, DlxError> {
    let pkg_name = get_pkg_name(cached_dir)?;
    let pkg_dir = cached_dir.join("node_modules").join(&pkg_name);
    let manifest = read_json(&pkg_dir.join("package.json"))?;
    let bins = get_bins_from_package_manifest::<CmdShimHost>(&manifest, &pkg_dir);

    match bins.as_slice() {
        [] => Err(DlxError::NoBin { package: pkg_name }),
        [bin] => Ok(bin.name.clone()),
        bins => {
            let manifest_name = manifest.get("name").and_then(Value::as_str).unwrap_or(&pkg_name);
            let scopeless_name = scopeless(manifest_name);
            if let Some(bin) = bins.iter().find(|bin| bin.name == scopeless_name) {
                return Ok(bin.name.clone());
            }
            let names = bins.iter().map(|bin| bin.name.as_str()).collect::<Vec<_>>().join(", ");
            Err(DlxError::MultipleBins { package: pkg_name, bins: names })
        }
    }
}

/// The first key of the installed manifest's `dependencies`.
fn get_pkg_name(cached_dir: &Path) -> Result<String, DlxError> {
    let mut manifest = read_json(&cached_dir.join("package.json"))?;
    // The manifest writer records a `runtime:` dependency (e.g.
    // `pnpm dlx node@runtime:26.4.0`) as `engines.runtime` on disk;
    // reify it back into the dependency map — the same conversion the
    // manifest reader applies — so the runtime is discoverable here.
    convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
    convert_engines_runtime_to_dependencies(&mut manifest, "engines", "dependencies");
    manifest
        .get("dependencies")
        .and_then(Value::as_object)
        .and_then(|deps| deps.keys().next())
        .cloned()
        .ok_or(DlxError::NoDep)
}

fn read_json(path: &Path) -> Result<Value, DlxError> {
    let text = fs::read_to_string(path)
        .map_err(|source| DlxError::ReadManifest { path: path.display().to_string(), source })?;
    parse_manifest(&text).map_err(|error| DlxError::ReadManifest {
        path: path.display().to_string(),
        source: error.into(),
    })
}

/// The package name with any `@scope/` prefix removed.
fn scopeless(pkg_name: &str) -> &str {
    if let Some(rest) = pkg_name.strip_prefix('@') {
        rest.split_once('/').map_or(pkg_name, |(_, name)| name)
    } else {
        pkg_name
    }
}

#[cfg(test)]
mod tests;
