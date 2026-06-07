use clap::Parser;
use pnpr::{Config, ConfigSource, LogConfig, LogFormat, default_cache_dir, serve};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "pnpr", version, about = "pnpm-compatible npm registry server")]
struct Args {
    /// Path to a verdaccio-shaped YAML config (storage, uplinks,
    /// packages, log). When omitted, the global `config.yaml` in
    /// pnpr's config dir (pnpm's config-dir rules, under `pnpr`) is
    /// used if it exists, otherwise the bundled default config.
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Address to bind to.
    #[arg(long, default_value = "127.0.0.1:4873")]
    listen: SocketAddr,

    /// Override the storage path from the loaded config (bundled or
    /// `-c`). Useful for tests and benchmarks that want their own
    /// storage directory without writing a custom YAML. Unless
    /// `--cache` is also given, the disposable proxy cache is
    /// re-derived as a subdirectory of this path.
    #[arg(long)]
    storage: Option<PathBuf>,

    /// Override the proxy-cache path — the disposable mirror of
    /// upstream registries plus the resolver's cache. Point
    /// it at separate, ephemeral disk to keep published packages and
    /// cached upstream content on different volumes.
    #[arg(long)]
    cache: Option<PathBuf>,

    /// URL clients should use to reach this server. Used when
    /// rewriting `dist.tarball` URLs in served packuments. Defaults
    /// to `http://<listen>`.
    #[arg(long)]
    public_url: Option<String>,

    /// Seconds before a cached packument is considered stale and
    /// refetched. When omitted, the loaded config's value wins.
    #[arg(long)]
    packument_ttl_secs: Option<u64>,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = Args::parse();
    let auto_path = Config::auto_config_path();
    let (mut config, source) = Config::resolve(
        args.config.as_deref(),
        auto_path.as_deref(),
        args.listen,
        args.public_url.clone(),
    )
    .map_err(|err| miette::miette!("{err}"))?;
    if let Some(storage) = args.storage {
        // The bundled config anchors auth state (htpasswd, tokens.db) next to the
        // config, which for the bundled default is the current directory. When a
        // caller serves from an explicit --storage dir (tests, benchmarks), keep
        // that state inside it so runs never write auth files into the working tree.
        if matches!(source, ConfigSource::Bundled) {
            if config.auth.htpasswd.file.is_some() {
                config.auth.htpasswd.file = Some(storage.join("htpasswd"));
            }
            if config.auth.tokens.file.is_some() {
                config.auth.tokens.file = Some(storage.join("tokens.db"));
            }
        }
        // Keep the cache co-located under the overridden storage dir so a
        // `--storage`-only run stays self-contained, unless the caller
        // pins the cache explicitly below.
        config.cache_storage = default_cache_dir(&storage);
        config.storage = storage;
    }
    if let Some(cache) = args.cache {
        config.cache_storage = cache;
    }
    if let Some(ttl_secs) = args.packument_ttl_secs {
        config.packument_ttl = Duration::from_secs(ttl_secs);
    }
    init_logging(&config.logs);
    log_config_source(&source);
    serve(config).await.map_err(|err| miette::miette!("{err}"))
}

/// Install the `tracing-subscriber` for this process based on the
/// resolved log config. `RUST_LOG` always wins over the YAML/CLI
/// level — same precedence Node ecosystem tools use for their
/// `LOG_LEVEL`/`DEBUG` env vars, and what existing pnpr
/// operators will already expect.
fn init_logging(logs: &LogConfig) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(logs.level.as_filter_directive()));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match logs.format {
        // `with_current_span(true)` keeps the per-request span's
        // `method`/`uri` fields attached to the single access event;
        // `with_span_list(false)` drops the redundant entered-span
        // array so each JSON line stays one flat access record.
        LogFormat::Json => builder.json().with_current_span(true).with_span_list(false).init(),
        LogFormat::Pretty => builder.compact().init(),
    }
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
