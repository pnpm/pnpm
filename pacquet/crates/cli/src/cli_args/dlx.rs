use crate::{State, cli_args::add::add_package};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pacquet_cmd_shim::{Host as CmdShimHost, get_bins_from_package_manifest};
use pacquet_config::Config;
use pacquet_crypto_hash::create_short_hash;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Run a package in a temporary environment.
///
/// Ports the single-package (non-recursive) path of pnpm's `dlx`
/// command from
/// <https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/dlx.ts>.
///
/// Deviations from pnpm, deferred until the surrounding infrastructure
/// lands:
/// - The cache key is built from the raw package specs rather than the
///   resolved package ids, so a floating spec such as `cowsay` is not
///   re-resolved until the cache entry expires.
/// - The interactive `approve-builds` prompt is not ported; transitive
///   build scripts follow the install's normal allow-list.
/// - Per-axis `--cpu` / `--os` / `--libc` overrides are not wired.
#[derive(Debug, Args)]
pub struct DlxArgs {
    /// The command to run, followed by its arguments.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,

    /// The package to install before running the command. May be
    /// repeated. When omitted, the command name is the package.
    #[clap(long = "package")]
    pub package: Vec<String>,

    /// Run the command inside of a shell. Uses `/bin/sh` on UNIX and
    /// `cmd.exe` on Windows.
    #[clap(long, short = 'c')]
    pub shell_mode: bool,
}

/// Errors from `pacquet dlx`.
///
/// Mirrors the error codes pnpm raises in `dlx.ts`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/dlx.ts>).
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

    #[display("Failed to read the installed manifest at {path}: {source}")]
    #[diagnostic(code(pacquet_cli::dlx_read_manifest))]
    ReadManifest {
        path: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to prepare the dlx cache directory {dir}: {source}")]
    #[diagnostic(code(pacquet_cli::dlx_cache))]
    Cache {
        dir: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to spawn command \"{command}\": {source}")]
    #[diagnostic(code(pacquet_cli::dlx_spawn))]
    Spawn {
        command: String,
        #[error(source)]
        source: std::io::Error,
    },
}

impl DlxArgs {
    /// Execute the subcommand. `dir` is the directory the spawned binary
    /// runs in (the process working directory); the package itself is
    /// installed into a cache directory under `config.cache_dir`.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        config: &'static mut Config,
    ) -> miette::Result<()> {
        let DlxArgs { command, package, shell_mode } = self;
        let Some((bin_command, args)) = command.split_first() else {
            return Err(DlxError::MissingCommand.into());
        };

        // pnpm: `pkgs = opts.package ?? [command]`. With `--package`, the
        // command names the bin to run; otherwise the command is also the
        // package. See dlx.ts:107.
        let pkgs: Vec<String> =
            if package.is_empty() { vec![bin_command.clone()] } else { package.clone() };

        // Read the config values needed before (and after) the install,
        // because the install path consumes `config` to anchor it at the
        // cache directory.
        let cache_dir = config.cache_dir.clone();
        let max_age = config.dlx_cache_max_age;
        let cache_key = create_cache_key(&pkgs, &config.registry);
        let extra_bin_paths = config.extra_bin_paths.clone();

        let dlx_command_cache_dir = cache_dir.join("dlx").join(&cache_key);
        fs::create_dir_all(&dlx_command_cache_dir).map_err(|source| DlxError::Cache {
            dir: dlx_command_cache_dir.display().to_string(),
            source,
        })?;
        let cache_link = dlx_command_cache_dir.join("pkg");

        let cached_dir = match get_valid_cache_dir(&cache_link, max_age, SystemTime::now()) {
            Some(dir) => dir,
            None => {
                let prepare_dir =
                    get_prepare_dir(&dlx_command_cache_dir, SystemTime::now(), std::process::id());
                install_into_cache::<Reporter>(&prepare_dir, &pkgs, config).await?;
                symlink_overwrite(&prepare_dir, &cache_link).map_err(|source| DlxError::Cache {
                    dir: cache_link.display().to_string(),
                    source,
                })?;
                prepare_dir
            }
        };

        let bins_dir = cached_dir.join("node_modules").join(".bin");
        let bin_name =
            if package.is_empty() { get_bin_name(&cached_dir)? } else { bin_command.clone() };

        run_bin(&bin_name, args, dir, bins_dir, &extra_bin_paths, shell_mode)
    }
}

/// Install `pkgs` into `prepare_dir` so their bins land in
/// `<prepare_dir>/node_modules/.bin`. Anchors `config` at the cache
/// directory (mirroring pnpm's `dir` / `lockfileDir` / `bin` overrides
/// at dlx.ts:180-184) and saves each package to `dependencies`.
async fn install_into_cache<Reporter: self::Reporter + 'static>(
    prepare_dir: &Path,
    pkgs: &[String],
    config: &'static mut Config,
) -> miette::Result<()> {
    fs::create_dir_all(prepare_dir)
        .map_err(|source| DlxError::Cache { dir: prepare_dir.display().to_string(), source })?;
    let manifest_path = prepare_dir.join("package.json");
    fs::write(&manifest_path, json!({ "name": "dlx", "version": "0.0.0" }).to_string())
        .map_err(|source| DlxError::Cache { dir: manifest_path.display().to_string(), source })?;

    config.modules_dir = prepare_dir.join("node_modules");
    config.virtual_store_dir = prepare_dir.join("node_modules").join(".pacquet");
    // The cache install is always fresh, so no lockfile is loaded from
    // the process working directory.
    config.lockfile = false;
    let config: &Config = config;

    for pkg in pkgs {
        let state = State::init(manifest_path.clone(), config, false)
            .wrap_err("initialize the dlx install state")?;
        add_package::<Reporter, _, _>(
            state,
            pkg,
            false,
            config.supported_architectures.clone(),
            || std::iter::once(DependencyGroup::Prod),
        )
        .await?;
    }
    Ok(())
}

/// Resolve and spawn the bin, prepending the cache's `node_modules/.bin`
/// (and `extraBinPaths`) to `PATH`. Mirrors dlx.ts:222-235.
fn run_bin(
    bin_name: &str,
    args: &[String],
    cwd: &Path,
    bins_dir: PathBuf,
    extra_bin_paths: &[PathBuf],
    shell_mode: bool,
) -> miette::Result<()> {
    let mut prepend = Vec::with_capacity(1 + extra_bin_paths.len());
    prepend.push(bins_dir);
    prepend.extend(extra_bin_paths.iter().cloned());
    let path = prepend_dirs_to_path(&prepend);

    let mut cmd = if shell_mode {
        let shell = pacquet_executor::select_shell(None, cfg!(windows))
            .expect("default shell selection never fails");
        let mut joined = vec![bin_name.to_string()];
        joined.extend(args.iter().cloned());
        let mut cmd = Command::new(&shell.program);
        cmd.args(&shell.args).arg(joined.join(" "));
        cmd
    } else {
        let program = which::which_in(bin_name, Some(&path), cwd)
            .map_err(|_| DlxError::CommandNotFound { command: bin_name.to_string() })?;
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    };

    cmd.current_dir(cwd);
    cmd.env_remove("PATH");
    cmd.env_remove("Path");
    cmd.env("PATH", &path);
    cmd.env("npm_config_user_agent", "pnpm");

    let status =
        cmd.status().map_err(|source| DlxError::Spawn { command: bin_name.to_string(), source })?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Build the dlx cache key. Ports the input composition of pnpm's
/// `createCacheKey`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/dlx.ts#L384-L410>):
/// the sorted package specs and sorted registries are hashed together.
/// pacquet keys on the raw specs (not resolved ids) and uses
/// [`create_short_hash`] rather than pnpm's full-length hex digest; the
/// dlx caches are not shared between the two implementations, so the key
/// format is not a cross-tool contract.
fn create_cache_key(pkgs: &[String], registry: &str) -> String {
    let mut sorted: Vec<&str> = pkgs.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let args = json!([sorted, [["default", registry]]]);
    create_short_hash(&args.to_string())
}

/// Return the cache target behind `cache_link` when it is a symlink whose
/// own mtime is within `max_age_minutes` of `now`. Ports `getValidCacheDir`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/dlx.ts#L412-L431>).
fn get_valid_cache_dir(
    cache_link: &Path,
    max_age_minutes: u64,
    now: SystemTime,
) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(cache_link).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let target = fs::canonicalize(cache_link).ok()?;
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
/// prepared in. Ports `getPrepareDir`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/dlx.ts#L433-L436>).
fn get_prepare_dir(cache_path: &Path, now: SystemTime, pid: u32) -> PathBuf {
    let millis = now.duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis()).unwrap_or(0);
    cache_path.join(format!("{millis:x}-{pid:x}"))
}

/// Determine the bin to run from the first installed dependency. Ports
/// `getBinName` + `getPkgName`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/dlx.ts#L247-L295>).
fn get_bin_name(cached_dir: &Path) -> Result<String, DlxError> {
    let pkg_name = get_pkg_name(cached_dir)?;
    let pkg_dir = cached_dir.join("node_modules").join(&pkg_name);
    let manifest = read_json(&pkg_dir.join("package.json"))?;
    let bins = get_bins_from_package_manifest::<CmdShimHost>(&manifest, &pkg_dir);

    match bins.as_slice() {
        [] => Err(DlxError::NoBin { package: pkg_name }),
        [bin] => Ok(bin.name.clone()),
        bins => {
            let scopeless = scopeless(&pkg_name);
            if let Some(bin) = bins.iter().find(|bin| bin.name == scopeless) {
                return Ok(bin.name.clone());
            }
            let names = bins.iter().map(|bin| bin.name.as_str()).collect::<Vec<_>>().join(", ");
            Err(DlxError::MultipleBins { package: pkg_name, bins: names })
        }
    }
}

/// The first key of the installed manifest's `dependencies`. Ports
/// `getPkgName` (dlx.ts:247-254).
fn get_pkg_name(cached_dir: &Path) -> Result<String, DlxError> {
    let manifest = read_json(&cached_dir.join("package.json"))?;
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
    serde_json::from_str(&text).map_err(|error| DlxError::ReadManifest {
        path: path.display().to_string(),
        source: error.into(),
    })
}

/// The package name with any `@scope/` prefix removed. Ports `scopeless`
/// (dlx.ts:297-302).
fn scopeless(pkg_name: &str) -> &str {
    if let Some(rest) = pkg_name.strip_prefix('@') {
        rest.split_once('/').map(|(_, name)| name).unwrap_or(pkg_name)
    } else {
        pkg_name
    }
}

fn prepend_dirs_to_path(dirs: &[PathBuf]) -> OsString {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut out = OsString::new();
    for (index, dir) in dirs.iter().enumerate() {
        if index > 0 {
            out.push(sep);
        }
        out.push(dir);
    }
    if let Some(current) = std::env::var_os("PATH")
        && !current.is_empty()
    {
        if !out.is_empty() {
            out.push(sep);
        }
        out.push(current);
    }
    out
}

/// Create a directory symlink at `link` pointing to `target`, replacing
/// any existing link. Mirrors pnpm's `symlinkDir(..., { overwrite: true })`.
fn symlink_overwrite(target: &Path, link: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(link).is_ok() {
        fs::remove_file(link).or_else(|_| fs::remove_dir_all(link))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link)
    }
}

#[cfg(test)]
mod tests;
