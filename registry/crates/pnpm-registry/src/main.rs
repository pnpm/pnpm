use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use pnpm_registry::{Config, serve};

#[derive(Debug, Parser)]
#[command(name = "pnpm-registry", version, about = "pnpm-compatible npm registry server")]
struct Args {
    /// Address to bind to.
    #[arg(long, default_value = "127.0.0.1:4873")]
    listen: SocketAddr,

    /// Upstream npm registry to proxy and cache from.
    #[arg(long, default_value = "https://registry.npmjs.org")]
    upstream: String,

    /// URL clients should use to reach this server. Used when
    /// rewriting `dist.tarball` URLs in cached packuments.
    #[arg(long)]
    public_url: Option<String>,

    /// Directory under which packuments and tarballs are cached.
    #[arg(long, default_value = "./storage")]
    cache_dir: PathBuf,

    /// Seconds before a cached packument is considered stale and
    /// refetched. Tarballs are cached indefinitely.
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
    let mut config = Config::new(args.listen, args.cache_dir);
    config.upstream = args.upstream;
    if let Some(url) = args.public_url {
        config.public_url = url;
    }
    config.packument_ttl = Duration::from_secs(args.packument_ttl_secs);

    serve(config).await.map_err(|err| miette::miette!("{err}"))
}
