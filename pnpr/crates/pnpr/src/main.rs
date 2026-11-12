use clap::{ArgMatches, CommandFactory as _, FromArgMatches as _, Parser, parser::ValueSource};
use pnpm_diagnostics::println;
use pnpr::{
    Command, Config, ConfigSource, ConfigSubcommand, LogConfig, RegistryError, ServerArgs,
    default_cache_dir, logging::log_subscriber, run_command, serve,
};
use pnpr_config::{BackendConfig, MetadataBackend, default_metadata_path};
use std::{io::IsTerminal, path::PathBuf};
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
    /// `-c`). A proxy cache or metadata database at its default path in
    /// the storage directory moves with it.
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
    let matches = Args::command().get_matches();
    let mut args = Args::from_arg_matches(&matches).unwrap_or_else(|err| err.exit());
    let auto_path = Config::auto_config_path();

    let server_args = match args.command.take() {
        None => args.server,
        Some(Command::Serve(server)) => {
            args.server.merge(without_env_values(server, matches.subcommand_matches("serve")))
        }
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
    apply_storage_overrides(&mut config, &mut args.paths);
    apply_server_overrides(&mut config, &server_args);

    init_logging(&config.logs);
    log_config_source(&source);
    warn_experimental_database(&config);
    warn_derived_public_url(&config);
    serve(config).await.map_err(|err| redacted_report(&err))
}

async fn run_maintenance(
    cmd: Command,
    mut args: Args,
    auto_path: Option<PathBuf>,
) -> miette::Result<()> {
    let (mut config, source) = Config::resolve_with_overrides(
        args.config.as_deref(),
        auto_path.as_deref(),
        args.server.listen,
        args.server.public_url.clone(),
        args.server.features.feature_overrides(),
    )
    .map_err(|err| miette::miette!("{err}"))?;
    apply_storage_overrides(&mut config, &mut args.paths);

    if matches!(cmd, Command::Config { action: ConfigSubcommand::Check }) {
        let report = pnpr::config_check::check_resolved_config(&mut config, &source)
            .map_err(|error| redacted_report(&error))?;
        println!("{}", pnpr::config_check::format_report(&report));
        return Ok(());
    }

    init_logging(&config.logs);
    log_config_source(&source);
    warn_experimental_database(&config);
    run_command(&config, cmd).await
}

/// `serve` reads the same environment variables as the top level, which
/// already holds their values. Taken from `serve` as well, they would replace
/// an explicit top-level flag in [`ServerArgs::merge`].
fn without_env_values(mut serve: ServerArgs, matches: Option<&ArgMatches>) -> ServerArgs {
    let from_env = |id: &str| {
        matches.is_some_and(|matches| matches.value_source(id) == Some(ValueSource::EnvVariable))
    };
    if from_env("listen") {
        serve.listen = None;
    }
    if from_env("public_url") {
        serve.public_url = None;
    }
    if from_env("packument_ttl") {
        serve.packument_ttl = None;
    }
    if from_env("osv") {
        serve.osv_options.osv = None;
    }
    if from_env("osv_db") {
        serve.osv_options.osv_db = None;
    }
    serve
}

fn apply_storage_overrides(config: &mut Config, paths: &mut StorageArgs) {
    if let Some(storage) = paths.storage.take() {
        // A cache at its default path lives in the storage directory, so it
        // moves with it. One the config places elsewhere stays there.
        if config.storage.cache_dir == default_cache_dir(&config.storage.hosted_dir) {
            config.storage.cache_dir = default_cache_dir(&storage);
        }
        // A database at the default path lives in the storage directory, so
        // it moves with it, whether or not the config spells the path out.
        // One anywhere else stays where it is.
        let default_metadata =
            MetadataBackend::Sqlite(default_metadata_path(&config.storage.hosted_dir));
        if config.storage.metadata == default_metadata {
            let moved = default_metadata_path(&storage);
            config.identity.backend = BackendConfig::Sqlite(moved.clone());
            config.storage.metadata = MetadataBackend::Sqlite(moved);
        }
        config.storage.hosted_dir = storage;
    }
    if let Some(cache) = paths.cache.take() {
        config.storage.cache_dir = cache;
    }
}

fn apply_server_overrides(config: &mut Config, server: &ServerArgs) {
    if let Some(ttl) = server.packument_ttl {
        config.http.packument_ttl = ttl;
    }
    if let Some(osv) = server.osv_options.osv {
        config.osv.enabled = osv;
    }
    if let Some(osv_db) = server.osv_options.osv_db.clone() {
        config.osv.path = Some(osv_db);
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

/// A server reachable beyond loopback without a public URL rewrites tarball
/// URLs to its bind address, which clients usually cannot reach.
fn warn_derived_public_url(config: &Config) {
    let listen = config.http.listen;
    if !listen.ip().is_loopback() && config.http.public_url == format!("http://{listen}") {
        tracing::warn!(
            %listen,
            "publicUrl is not set; tarball URLs point at the bind address. Set `publicUrl` to the URL clients use",
        );
    }
}

fn warn_experimental_database(config: &Config) {
    let mysql = matches!(config.storage.metadata, MetadataBackend::Mysql(_))
        || matches!(config.identity.backend, BackendConfig::Mysql(_));
    if mysql {
        tracing::warn!(
            "MySQL support is experimental; use SQLite, libSQL, or PostgreSQL for production data",
        );
    }
}

#[cfg(test)]
mod tests;
