use crate::cli_args::exec::{MakeEnv, make_env};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_cmd_shim::{Host as CmdShimHost, get_bins_from_package_manifest};
use pacquet_config::{Config, Host};
use pacquet_crypto_hash::create_short_hash;
use pacquet_executor::select_shell;
use pacquet_network::ThrottledClient;
use pacquet_package_manager::{Install, ResolvedPackages};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::Reporter;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime},
};

#[derive(Debug, Args)]
pub struct DlxArgs {
    /// The package(s) to install before running the command. When set,
    /// the positional command names the binary to run from those
    /// packages.
    #[clap(long = "package")]
    pub package: Vec<String>,

    /// Package names allowed to run lifecycle (build) scripts during the
    /// dlx install.
    #[clap(long = "allow-build")]
    pub allow_build: Vec<String>,

    /// Run the command inside a shell.
    #[clap(short = 'c', long = "shell-mode")]
    pub shell_mode: bool,

    /// The command to run, followed by its arguments.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// Error type of [`DlxArgs::run`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum DlxError {
    #[display("'pacquet dlx' requires a command or a --package to run")]
    #[diagnostic(code(ERR_PNPM_DLX_NO_COMMAND))]
    NoCommand,

    #[display(
        "dlx does not yet support the non-registry specifier \"{spec}\" (git / tarball / file)"
    )]
    #[diagnostic(code(pacquet_cli::dlx_unsupported_spec))]
    UnsupportedSpec { spec: String },

    #[display("dlx was unable to find the installed dependency in \"dependencies\"")]
    #[diagnostic(code(ERR_PNPM_DLX_NO_DEP))]
    NoDependency,

    #[display("No binaries found in {package}")]
    #[diagnostic(code(ERR_PNPM_DLX_NO_BIN))]
    NoBin { package: String },

    #[display("Could not determine executable to run. {package} has multiple binaries: {bins}")]
    #[diagnostic(
        code(ERR_PNPM_DLX_MULTIPLE_BINS),
        help("Use --package={package} dlx <bin> to pick one of: {bins}")
    )]
    MultipleBins { package: String, bins: String },

    #[display("Failed to spawn `{command}`: {source}")]
    #[diagnostic(code(pacquet_cli::dlx_spawn))]
    Spawn {
        command: String,
        #[error(source)]
        source: std::io::Error,
    },
}

impl DlxArgs {
    /// Resolve, install (into a cache dir), and run a package in a
    /// throwaway environment. Ports the single-package, non-interactive
    /// path of upstream's `dlx` handler at
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/dlx.ts>.
    ///
    /// `config` is the user's resolved config; its `registry`,
    /// `cache_dir`, and `dlx_cache_max_age` drive the cache. The install
    /// itself runs under a fresh config anchored to the cache dir so the
    /// throwaway `node_modules` never touches the caller's project.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        config: &'static Config,
    ) -> miette::Result<()> {
        let DlxArgs { package, allow_build, shell_mode, command } = self;

        let package_provided = !package.is_empty();
        let (bin_arg, args) = match command.split_first() {
            Some((first, rest)) => (Some(first.clone()), rest.to_vec()),
            None => (None, Vec::new()),
        };
        if !package_provided && bin_arg.is_none() {
            return Err(DlxError::NoCommand.into());
        }

        // `pkgs` are the specs to install; when `--package` is absent the
        // single positional doubles as the package spec.
        let pkgs: Vec<String> =
            if package_provided { package } else { vec![bin_arg.clone().expect("checked above")] };

        let registries = registries_map(config);
        let cache_key = create_cache_key(&pkgs, &registries, &allow_build);
        let dlx_command_cache_dir = config.cache_dir.join("dlx").join(&cache_key);
        fs::create_dir_all(&dlx_command_cache_dir)
            .into_diagnostic()
            .wrap_err("creating the dlx cache directory")?;
        // Canonicalize so the prepare dir carries no `..` segments. A
        // relative `cacheDir` (e.g. `../pnpm-cache`) would otherwise leave
        // the install's lexical workspace-root walk passing *through* the
        // caller's project dir and mistaking it for the dlx workspace.
        let dlx_command_cache_dir = dunce::canonicalize(&dlx_command_cache_dir)
            .into_diagnostic()
            .wrap_err("canonicalizing the dlx cache directory")?;
        let cache_link = dlx_command_cache_dir.join("pkg");

        let cached_dir = match valid_cache_dir(&cache_link, config.dlx_cache_max_age) {
            Some(dir) => dir,
            None => {
                let prepare_dir = dlx_command_cache_dir.join(prepare_dir_name());
                install_into::<Reporter>(dir, &prepare_dir, &pkgs, &allow_build).await?;
                // Best-effort: a parallel dlx process may have raced us to
                // the link. Either link is equally fresh, so ignore the
                // failure and run from our own prepare dir.
                let _ = pacquet_fs::force_symlink_dir(&prepare_dir, &cache_link);
                prepare_dir
            }
        };

        // With `--package`, the positional is the binary to run; without
        // it, the binary is derived from the single installed dependency.
        let bin_name = if package_provided {
            bin_arg.ok_or(DlxError::NoCommand)?
        } else {
            get_bin_name(&cached_dir)?
        };

        run_bin(&cached_dir, &bin_name, &args, shell_mode, config)
    }
}

/// Install the requested specs into `prepare_dir` as a self-contained
/// project (its own `node_modules` and lockfile, no global virtual
/// store), so the cache directory can be symlinked and reused.
async fn install_into<Reporter: self::Reporter + 'static>(
    dir: &Path,
    prepare_dir: &Path,
    pkgs: &[String],
    allow_build: &[String],
) -> miette::Result<()> {
    fs::create_dir_all(prepare_dir)
        .into_diagnostic()
        .wrap_err("creating the dlx prepare directory")?;

    let mut dependencies = serde_json::Map::new();
    let mut aliases: Vec<String> = Vec::new();
    for spec in pkgs {
        let parsed = parse_wanted_dependency(spec);
        let Some(alias) = parsed.alias else {
            return Err(DlxError::UnsupportedSpec { spec: spec.clone() }.into());
        };
        let bare = parsed.bare_specifier.unwrap_or_else(|| "latest".to_string());
        dependencies.insert(alias.clone(), Value::String(bare));
        aliases.push(alias);
    }

    let manifest_value = json!({
        "name": "dlx-tmp",
        "version": "0.0.0",
        "dependencies": Value::Object(dependencies),
    });
    fs::write(
        prepare_dir.join("package.json"),
        serde_json::to_string_pretty(&manifest_value).expect("serialize manifest"),
    )
    .into_diagnostic()
    .wrap_err("writing the dlx package.json")?;

    // Build the install config from the caller's working dir so the
    // user's registry / auth (`.npmrc`) and `storeDir` / `cacheDir`
    // (`pnpm-workspace.yaml`) are honored, then redirect the install
    // location into the throwaway cache dir. A per-project virtual store
    // (GVS off) keeps the cache dir self-contained.
    let mut cfg = Config::default()
        .current::<Host>(dir)
        .map_err(miette::Report::new)
        .wrap_err("loading dlx install config")?;
    cfg.modules_dir = prepare_dir.join("node_modules");
    cfg.virtual_store_dir = prepare_dir.join("node_modules").join(".pnpm");
    cfg.enable_global_virtual_store = false;
    // The throwaway install is a standalone project, not part of the
    // caller's workspace — drop any workspace association picked up from
    // the caller's `pnpm-workspace.yaml`.
    cfg.workspace_dir = None;
    // Allow the requested packages (and their aliases) to run build
    // scripts during the install, mirroring pnpm's dlx `allowBuilds`.
    for name in aliases.iter().chain(allow_build.iter()) {
        cfg.allow_builds.insert(name.clone(), true);
    }
    let cfg: &'static Config = Config::leak(cfg);

    let manifest = PackageManifest::from_path(prepare_dir.join("package.json"))
        .wrap_err("reading the dlx package.json")?;

    let http_client = Arc::new(
        ThrottledClient::for_installs(&cfg.proxy, &cfg.tls, &cfg.tls_by_uri)
            .map_err(miette::Report::new)
            .wrap_err("building the dlx http client")?,
    );
    let tarball_mem_cache = Arc::new(pacquet_tarball::MemCache::new());
    let resolved_packages = ResolvedPackages::new();

    Install {
        tarball_mem_cache,
        http_client: &http_client,
        http_client_arc: Arc::clone(&http_client),
        config: cfg,
        manifest: &manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: cfg.trust_lockfile,
        supported_architectures: None,
        node_linker: cfg.node_linker,
        resolved_packages: &resolved_packages,
    }
    .run::<Reporter>()
    .await
    .wrap_err("installing the dlx package")
}

/// Spawn the resolved binary with the cache dir's `node_modules/.bin`
/// prepended to PATH, in the caller's working directory.
fn run_bin(
    cached_dir: &Path,
    bin_name: &str,
    args: &[String],
    shell_mode: bool,
    config: &Config,
) -> miette::Result<()> {
    let env = make_env(MakeEnv {
        dir: cached_dir,
        extra_bin_paths: &config.extra_bin_paths,
        node_options: config.node_options.as_deref(),
        package_name: None,
        user_agent: "pnpm",
    });

    let status = if shell_mode {
        let mut line = bin_name.to_string();
        for arg in args {
            line.push(' ');
            line.push_str(arg);
        }
        let shell = select_shell(None, cfg!(windows)).map_err(miette::Report::new)?;
        Command::new(&shell.program)
            .args(&shell.args)
            .arg(&line)
            .env_clear()
            .envs(&env)
            .status()
            .map_err(|source| DlxError::Spawn { command: line, source })?
    } else {
        Command::new(bin_name)
            .args(args)
            .env_clear()
            .envs(&env)
            .status()
            .map_err(|source| DlxError::Spawn { command: bin_name.to_string(), source })?
    };

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Build the `{ "default": registry, <alias>: url, … }` map pnpm feeds
/// into the cache key.
fn registries_map(config: &Config) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("default".to_string(), config.registry.clone());
    for (name, url) in &config.named_registries {
        map.insert(name.clone(), url.clone());
    }
    map
}

/// Hash the install request into a cache key. Mirrors the *inputs* of
/// upstream's `createCacheKey` (sorted specs, sorted registries, and the
/// optional `allowBuild` list), but keys on the raw specs rather than the
/// resolved package ids and uses pacquet's 32-char `create_short_hash` —
/// the dlx cache is internal, not shared with pnpm. Resolving to ids
/// before hashing (so floating tags pick up new versions before the TTL)
/// is a follow-up once a standalone resolver entry is wired in.
fn create_cache_key(
    pkgs: &[String],
    registries: &BTreeMap<String, String>,
    allow_build: &[String],
) -> String {
    let mut sorted_pkgs: Vec<&String> = pkgs.iter().collect();
    sorted_pkgs.sort();
    let registry_pairs: Vec<(&String, &String)> = registries.iter().collect();

    let mut args = vec![json!(sorted_pkgs), json!(registry_pairs)];
    if !allow_build.is_empty() {
        let mut sorted_allow: Vec<&String> = allow_build.iter().collect();
        sorted_allow.sort();
        args.push(json!({ "allowBuild": sorted_allow }));
    }
    let serialized = serde_json::to_string(&args).expect("serialize cache key inputs");
    create_short_hash(&serialized)
}

/// A unique throwaway directory name within the cache key dir.
fn prepare_dir_name() -> String {
    let millis =
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("{millis:x}-{:x}", std::process::id())
}

/// Resolve `cache_link` to its target if it is a symlink whose age is
/// within `max_age_minutes`. Mirrors upstream's `getValidCacheDir`.
fn valid_cache_dir(cache_link: &Path, max_age_minutes: u64) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(cache_link).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let target = fs::canonicalize(cache_link).ok()?;
    let modified = meta.modified().ok()?;
    let max_age = Duration::from_secs(max_age_minutes.saturating_mul(60));
    let expires_at = modified.checked_add(max_age)?;
    (expires_at >= SystemTime::now()).then_some(target)
}

/// Read the first dependency name from the installed cache dir's
/// manifest. Mirrors upstream's `getPkgName`.
fn get_pkg_name(cached_dir: &Path) -> Result<String, DlxError> {
    let text =
        fs::read_to_string(cached_dir.join("package.json")).map_err(|_| DlxError::NoDependency)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| DlxError::NoDependency)?;
    value
        .get("dependencies")
        .and_then(Value::as_object)
        .and_then(|deps| deps.keys().next())
        .cloned()
        .ok_or(DlxError::NoDependency)
}

/// Determine which binary to run from the installed dependency. Mirrors
/// upstream's `getBinName`.
fn get_bin_name(cached_dir: &Path) -> Result<String, DlxError> {
    let pkg_name = get_pkg_name(cached_dir)?;
    let pkg_dir = cached_dir.join("node_modules").join(&pkg_name);
    let manifest_text = fs::read_to_string(pkg_dir.join("package.json"))
        .map_err(|_| DlxError::NoBin { package: pkg_name.clone() })?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|_| DlxError::NoBin { package: pkg_name.clone() })?;

    let bins = get_bins_from_package_manifest::<CmdShimHost>(&manifest, &pkg_dir);
    match bins.as_slice() {
        [] => Err(DlxError::NoBin { package: pkg_name }),
        [only] => Ok(only.name.clone()),
        many => {
            let scopeless_name = scopeless(&pkg_name);
            if let Some(default) = many.iter().find(|b| b.name == scopeless_name) {
                return Ok(default.name.clone());
            }
            let names = many.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(", ");
            Err(DlxError::MultipleBins { package: pkg_name, bins: names })
        }
    }
}

/// Strip a leading `@scope/` from a package name. Mirrors upstream's
/// `scopeless`.
fn scopeless(pkg_name: &str) -> String {
    if let Some(rest) = pkg_name.strip_prefix('@') {
        rest.split_once('/').map_or_else(|| pkg_name.to_string(), |(_, name)| name.to_string())
    } else {
        pkg_name.to_string()
    }
}

#[cfg(test)]
mod tests;
