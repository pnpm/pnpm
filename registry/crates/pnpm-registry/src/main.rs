use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use pnpm_registry::{Config, serve};

#[derive(Debug, Parser)]
#[command(name = "pnpm-registry", version, about = "pnpm-compatible npm registry server")]
struct Args {
    /// Path to a verdaccio-shaped YAML config (storage, uplinks,
    /// packages). When omitted, the bundled default config is used.
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
    /// refetched.
    #[arg(long, default_value_t = 300)]
    packument_ttl_secs: u64,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let mut config = match args.config.as_deref() {
        Some(path) => Config::from_yaml(path, args.listen, args.public_url.clone())
            .map_err(|err| miette::miette!("load {}: {err}", path.display()))?,
        None => Config::from_default_yaml(Path::new("."), args.listen, args.public_url.clone()),
    };
    if let Some(storage) = args.storage {
        config.storage = storage;
    }
    config.packument_ttl = Duration::from_secs(args.packument_ttl_secs);

    serve(config).await.map_err(|err| miette::miette!("{err}"))
}
