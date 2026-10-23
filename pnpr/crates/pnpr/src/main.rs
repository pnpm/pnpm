use clap::Parser;
use pnpm_diagnostics::println;
use pnpr::{
    Command, Config, ConfigSource, ConfigSubcommand, LogConfig, RegistryError, ServerArgs,
    default_cache_dir, logging::log_subscriber, run_command, serve,
};
use std::{
    io::IsTerminal,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tracing_subscriber::{EnvFilter, util::SubscriberInitExt as _};

#[derive(Debug, Parser)]
#[command(name = "pnpr", version, about = "pnpm-compatible npm registry server")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the YAML config. When omitted, the global `config.yaml` in
    /// pnpr's config dir (pnpm's config-dir rules, under `pnpr`) is
    /// used if it exists, otherwise the bundled default config.
    #[arg(short = 'c', long, env = "PNPR_CONFIG", global = true)]
    config: Option<PathBuf>,

    #[command(flatten)]
    paths: StorageArgs,

    #[command(flatten)]
    server: ServerArgs,
}

#[derive(Debug, Default, Clone, clap::Args)]
struct StorageArgs {
    /// Override the storage path from the loaded config (bundled or
    /// `-c`). Unless
    /// `--cache` is also given, the disposable proxy cache is
    /// re-derived as a subdirectory of this path.
    #[arg(long, env = "PNPR_STORAGE", global = true)]
    storage: Option<PathBuf>,

    /// Override the proxy-cache path — the disposable mirror of
    /// upstream registries plus the resolver's cache. Point
    /// it at separate, ephemeral disk to keep published packages and
    /// cached upstream content on different volumes.
    #[arg(long, env = "PNPR_CACHE", global = true)]
    cache: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let mut args = Args::parse();
    let auto_path = Config::auto_config_path();

    let server_args = match args.command.take() {
        None => args.server,
        Some(Command::Serve(server)) => args.server.merge(server),
        Some(cmd) => return run_maintenance(cmd, args, auto_path).await,
    };

    let overrides = server_args.features.feature_overrides();
    let (mut config, source) = Config::resolve_with_overrides(
        args.config.as_deref(),
        auto_path.as_deref(),
        server_args.listen,
        server_args.public_url.clone(),
        overrides,
    )
    .map_err(|err| miette::miette!("{err}"))?;
    apply_storage_overrides(&mut config, &mut args.paths, &source);
    apply_server_overrides(&mut config, &server_args);

    init_logging(&config.logs);
    log_config_source(&source);
    serve(config).await.map_err(|err| redacted_report(&err))
}

async fn run_maintenance(
    cmd: Command,
    mut args: Args,
    auto_path: Option<PathBuf>,
) -> miette::Result<()> {
    let default_listen: SocketAddr = Config::DEFAULT_LISTEN.parse().unwrap();
    let (mut config, source) = Config::resolve_with_overrides(
        args.config.as_deref(),
        auto_path.as_deref(),
        default_listen,
        None,
        args.server.features.feature_overrides(),
    )
    .map_err(|err| miette::miette!("{err}"))?;
    apply_storage_overrides(&mut config, &mut args.paths, &source);

    if matches!(cmd, Command::ConfigCheck | Command::Config { action: ConfigSubcommand::Check }) {
        let report = pnpr::config_check::check_resolved_config(&config, &source);
        println!("{}", pnpr::config_check::format_report(&report));
        return Ok(());
    }

    init_logging(&config.logs);
    log_config_source(&source);
    run_command(&config, cmd).await
}

fn apply_storage_overrides(config: &mut Config, paths: &mut StorageArgs, source: &ConfigSource) {
    if let Some(storage) = paths.storage.take() {
        relocate_bundled_auth_state(config, &storage, source);
        config.storage.cache_dir = default_cache_dir(&storage);
        config.storage.hosted_dir = storage;
    }
    if let Some(cache) = paths.cache.take() {
        config.storage.cache_dir = cache;
    }
}

fn apply_server_overrides(config: &mut Config, server: &ServerArgs) {
    if let Some(ttl) = server.packument_ttl {
        config.http.packument_ttl = ttl;
    } else if let Ok(raw) = std::env::var("PNPR_PACKUMENT_TTL_SECS")
        && let Some(d) = pnpr_config::parse_interval(&raw)
    {
        config.http.packument_ttl = d;
    }
    if let Some(osv) = server.osv_options.osv {
        config.osv.enabled = osv;
    }
    if let Some(osv_db) = server.osv_options.osv_db.clone() {
        config.osv.path = Some(osv_db);
    }
}

/// The bundled config anchors auth state (htpasswd, tokens.db) next to the
/// config, which for the bundled default is the current directory. When a
/// caller serves from an explicit `--storage` dir (tests, benchmarks), keep
/// that state inside it so runs never write auth files into the working tree.
fn relocate_bundled_auth_state(config: &mut Config, storage: &Path, source: &ConfigSource) {
    if !matches!(source, ConfigSource::Bundled) {
        return;
    }
    if config.identity.auth.htpasswd.file.is_some() {
        config.identity.auth.htpasswd.file = Some(storage.join("htpasswd"));
    }
    if config.identity.auth.tokens.file.is_some() {
        config.identity.auth.tokens.file = Some(storage.join("tokens.db"));
    }
}

fn redacted_report(err: &RegistryError) -> miette::Report {
    let message = err.log_message();
    miette::miette!("{message}")
}

/// Install the `tracing-subscriber` for this process based on the
/// resolved log config. `RUST_LOG` always wins over the YAML/CLI
/// level — same precedence Node ecosystem tools use for their
/// `LOG_LEVEL`/`DEBUG` env vars, and what existing pnpr
/// operators will already expect.
fn init_logging(logs: &LogConfig) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(logs.level.as_filter_directive()));
    // Emit ANSI colors only to an interactive terminal — never when stdout is
    // redirected to a file or pipe (e.g. the benchmark's mock logs), where the
    // escape codes are just noise that breaks downstream log parsing. The
    // writer is pinned to stdout explicitly so the `is_terminal` probe always
    // inspects the stream the subscriber actually writes to.
    log_subscriber(logs, filter, std::io::stdout, std::io::stdout().is_terminal()).init();
    if !logs.sink_is_supported() {
        tracing::warn!(
            sink = %logs.sink,
            "unsupported `log.type`; only `stdout` is implemented — logging to stdout",
        );
    }
}

fn log_config_source(source: &ConfigSource) {
    match source {
        ConfigSource::Cli(path) => {
            tracing::info!(path = %path.display(), "loaded config from --config");
        }
        ConfigSource::DefaultPath(path) => {
            tracing::info!(path = %path.display(), "loaded config from default path");
        }
        ConfigSource::Bundled => tracing::info!("loaded bundled default config"),
    }
}

#[cfg(test)]
mod tests;
