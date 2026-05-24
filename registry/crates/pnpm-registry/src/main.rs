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

    /// Upstream npm registry to proxy and cache from. Ignored when
    /// `--static` is set.
    #[arg(long, default_value = "https://registry.npmjs.org")]
    upstream: String,

    /// Storage directory (verdaccio-shaped). In proxy mode this
    /// doubles as the cache; in static mode it's the source of truth.
    #[arg(long, default_value = "./storage")]
    storage: PathBuf,

    /// Serve `--storage` verbatim with no upstream — useful for
    /// running against a pre-populated verdaccio store (e.g.
    /// `@pnpm/registry-mock`'s `registry/storage-cache`).
    #[arg(long = "static")]
    static_serve: bool,

    /// URL clients should use to reach this server. Used when
    /// rewriting `dist.tarball` URLs in served packuments.
    #[arg(long)]
    public_url: Option<String>,

    /// Seconds before a cached packument is considered stale and
    /// refetched. Ignored in static mode.
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
    let mut config = if args.static_serve {
        Config::static_serve(args.listen, args.storage)
    } else {
        let mut config = Config::proxy(args.listen, args.storage);
        config.upstream = Some(args.upstream);
        config
    };
    if let Some(url) = args.public_url {
        config.public_url = url;
    }
    config.packument_ttl = Duration::from_secs(args.packument_ttl_secs);

    serve(config).await.map_err(|err| miette::miette!("{err}"))
}
