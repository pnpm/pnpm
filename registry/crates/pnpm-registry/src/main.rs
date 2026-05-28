use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use pnpm_registry::{Config, ConfigSource, LogConfig, LogFormat, serve};

#[derive(Debug, Parser)]
#[command(name = "pnpm-registry", version, about = "pnpm-compatible npm registry server")]
struct Args {
    /// Path to a verdaccio-shaped YAML config (storage, uplinks,
    /// packages, log). When omitted, `~/.config/pnpm-registry/config.yaml`
    /// is used if it exists, otherwise the bundled default config.
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Address to bind to.
    #[arg(long, default_value = "127.0.0.1:4873")]
    listen: SocketAddr,

    /// Override the storage path from the loaded config (bundled or
    /// `-c`). Useful for tests and benchmarks that want their own
    /// cache directory without writing a custom YAML.
    #[arg(long)]
    storage: Option<PathBuf>,

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
    let auto_path = Config::auto_config_path(home::home_dir().as_deref());
    let (mut config, source) = Config::resolve(
        args.config.as_deref(),
        auto_path.as_deref(),
        args.listen,
        args.public_url.clone(),
    )
    .map_err(|err| miette::miette!("{err}"))?;
    if let Some(storage) = args.storage {
        config.storage = storage;
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
/// `LOG_LEVEL`/`DEBUG` env vars, and what existing pnpm-registry
/// operators will already expect.
fn init_logging(logs: &LogConfig) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(logs.level.as_filter_directive()));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match logs.format {
        LogFormat::Json => builder.json().with_current_span(false).with_span_list(false).init(),
        LogFormat::Pretty => builder.compact().init(),
    }
}

fn log_config_source(source: &ConfigSource) {
    match source {
        ConfigSource::Cli(path) => {
            tracing::info!(path = %path.display(), "loaded config from --config")
        }
        ConfigSource::DefaultPath(path) => {
            tracing::info!(path = %path.display(), "loaded config from default path")
        }
        ConfigSource::Bundled => tracing::info!("loaded bundled default config"),
    }
}
