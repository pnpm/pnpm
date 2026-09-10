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
use cache::{
    command_cache_dir, get_valid_cache_dir, prepare_cache_dir, read_json, resolve_catalog_specs,
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
use provision::{ProvisionedTool, provisioned_tool, run_package_manager, run_runtime};
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
        let supported_architectures =
            SupportedArchitecturesArgs { cpu: self.cpu, os: self.os, libc: self.libc };
        let Some((bin_command, args)) = self.command.split_first() else {
            return Err(DlxError::MissingCommand.into());
        };

        let env = SpawnEnv::from_config(config, dir);
        let spawn = env.spawn(self.shell_mode);

        if let Some(tool) = provisioned_tool(&self.package, bin_command) {
            return run_provisioned::<Reporter>(tool, config, bin_command, args, &spawn).await;
        }

        // `pkgs = package ?? [command]`. With `--package`, the command
        // names the bin to run; otherwise the command is also the package.
        let pkgs: Vec<String> =
            if self.package.is_empty() { vec![bin_command.clone()] } else { self.package.clone() };
        // Resolved here rather than in the install below so the catalog's
        // version also feeds the cache key: two callers whose catalogs pin
        // different versions of the same package must not share a cache
        // entry.
        let pkgs = resolve_catalog_specs(&pkgs, config)?;

        // The effective (post-`--cpu`/`--os`/`--libc`) architecture set
        // is part of the cache key: it changes which platform-tagged
        // optional dependencies get installed, so two invocations that
        // differ only by architecture must not share a cache entry.
        let dlx_command_cache_dir =
            command_cache_dir(config, &pkgs, &self.allow_build, &supported_architectures)?;
        let cache_link = dlx_command_cache_dir.join("pkg");

        let cached_dir =
            match get_valid_cache_dir(&cache_link, config.dlx_cache_max_age, SystemTime::now()) {
                Some(cached_dir) => cached_dir,
                None => {
                    prepare_cache_dir::<Reporter>(
                        &dlx_command_cache_dir,
                        &cache_link,
                        &pkgs,
                        &self.allow_build,
                        &supported_architectures,
                        config,
                    )
                    .await?
                }
            };

        let bin_name =
            if self.package.is_empty() { get_bin_name(&cached_dir)? } else { bin_command.clone() };

        run_bin(
            DlxProgram::Named(&bin_name),
            args,
            vec![cached_dir.join("node_modules").join(".bin")],
            &spawn,
        )
    }
}

/// The config values every dlx spawn applies, read before the install path
/// consumes `config` to anchor it at the cache directory.
struct SpawnEnv {
    cwd: PathBuf,
    extra_bin_paths: Vec<PathBuf>,
    extra_env: HashMap<String, String>,
    user_agent: String,
}

impl SpawnEnv {
    fn from_config(config: &Config, dir: &Path) -> Self {
        let mut extra_env = config.extra_env.clone();
        // The GVS resolution env injected by `Config::current` points at
        // the *invoking* project's node_modules; the dlx tool runs from
        // its own self-contained cache (GVS forced off for the cache
        // install), so inheriting it would let the tool resolve phantom
        // deps from the caller's tree.
        extra_env.remove("NODE_PATH");
        extra_env.remove("NODE_OPTIONS");
        SpawnEnv {
            // The dlx command runs in the process working directory
            // (`cwd: process.cwd()`), independent of `--dir`.
            cwd: std::env::current_dir().unwrap_or_else(|_| dir.to_path_buf()),
            extra_bin_paths: config.extra_bin_paths.clone(),
            extra_env,
            user_agent: config.user_agent.clone(),
        }
    }

    fn spawn(&self, shell_mode: bool) -> DlxSpawn<'_> {
        DlxSpawn {
            cwd: &self.cwd,
            extra_bin_paths: &self.extra_bin_paths,
            extra_env: &self.extra_env,
            user_agent: &self.user_agent,
            shell_mode,
        }
    }
}

async fn run_provisioned<Reporter: self::Reporter + 'static>(
    tool: ProvisionedTool<'_>,
    config: &'static Config,
    bin_command: &str,
    args: &[String],
    spawn: &DlxSpawn<'_>,
) -> miette::Result<()> {
    match tool {
        ProvisionedTool::PackageManager { pm, version_spec, spec, bin } => {
            run_package_manager::<Reporter>(config, pm, version_spec, spec, bin, args, spawn).await
        }
        ProvisionedTool::Runtime { name, version_spec } => {
            run_runtime(&config.state_dir, name, version_spec, bin_command, args, spawn).await
        }
    }
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
    let mut prepend = bin_dirs;
    prepend.extend(spawn.extra_bin_paths.iter().cloned());
    let path = prepend_dirs_to_path(&prepend).map_err(DlxError::from)?;

    let mut cmd = if spawn.shell_mode {
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
            DlxProgram::Named(name) => which::which_in(name, Some(&path), spawn.cwd)
                .map_err(|_| DlxError::CommandNotFound { command: name.to_string() })?,
            DlxProgram::Provisioned { executable, .. } => executable.to_path_buf(),
        };
        let mut cmd = Command::new(executable);
        cmd.args(args);
        cmd
    };

    cmd.current_dir(spawn.cwd);
    // `updateConfig`-provided env, applied first so pnpm's own keys win
    // on conflict (matching `exec`'s spawn and TS `makeEnv`). dlx does
    // not run the `updateConfig` hook, so this is currently always
    // empty; wired for uniformity with the other spawn sites and so it
    // works if that changes.
    cmd.envs(spawn.extra_env);
    set_command_path(&mut cmd, &path);
    cmd.env("npm_config_user_agent", spawn.user_agent);

    let status = pnpm_executor::spawn_child(&mut cmd, None)
        .and_then(|mut child| child.wait())
        .map_err(|source| DlxError::Spawn { command: program.command().to_string(), source })?;
    if !status.success() {
        pnpm_executor::exit_like(pnpm_executor::ScriptExit::Process(status));
    }
    Ok(())
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

mod cache;

mod provision;
