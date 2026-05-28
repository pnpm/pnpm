use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use pnpm_registry::{Config, LogConfig, LogFormat, serve};

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

/// Where the config came from. Used after the subscriber is up to
/// log the resolved path (or note that the bundled default applies).
enum ConfigSource {
    Cli(PathBuf),
    DefaultPath(PathBuf),
    Bundled,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = Args::parse();
    let (config, source) = load_config(&args)?;
    init_logging(&config.logs);
    log_config_source(&source);
    serve(config).await.map_err(|err| miette::miette!("{err}"))
}

fn load_config(args: &Args) -> miette::Result<(Config, ConfigSource)> {
    let (mut config, source) = match args.config.as_deref() {
        Some(path) => {
            let displayed = path.display();
            let config = Config::from_yaml(path, args.listen, args.public_url.clone())
                .map_err(|err| miette::miette!("load {displayed}: {err}"))?;
            (config, ConfigSource::Cli(path.to_path_buf()))
        }
        None => match default_config_path() {
            Some(path) => {
                let displayed = path.display();
                let config = Config::from_yaml(&path, args.listen, args.public_url.clone())
                    .map_err(|err| miette::miette!("load {displayed}: {err}"))?;
                (config, ConfigSource::DefaultPath(path))
            }
            None => (
                Config::from_default_yaml(Path::new("."), args.listen, args.public_url.clone()),
                ConfigSource::Bundled,
            ),
        },
    };
    if let Some(storage) = args.storage.clone() {
        config.storage = storage;
    }
    if let Some(ttl_secs) = args.packument_ttl_secs {
        config.packument_ttl = Duration::from_secs(ttl_secs);
    }
    Ok((config, source))
}

/// `$HOME/.config/pnpm-registry/config.yaml` when it exists.
/// Returns `None` if the file is absent or `HOME` is unset — the
/// caller falls back to the bundled config in that case.
fn default_config_path() -> Option<PathBuf> {
    Config::auto_config_path(home::home_dir().as_deref())
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
