use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// Runtime configuration for the pnpm registry server.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub listen: SocketAddr,
    /// Upstream registry to proxy and cache from.
    pub upstream: String,
    /// URL clients should use to reach this server. Used to rewrite
    /// `dist.tarball` URLs in cached packuments so tarball requests
    /// flow through (and get cached by) this server.
    pub public_url: String,
    /// Directory under which packuments and tarballs are cached.
    pub cache_dir: PathBuf,
    /// How long a cached packument is considered fresh before it is
    /// re-fetched from the upstream. Tarballs are content-addressed by
    /// version and are cached indefinitely.
    pub packument_ttl: Duration,
}

impl Config {
    pub fn new(listen: SocketAddr, cache_dir: PathBuf) -> Self {
        let public_url = format!("http://{listen}");
        Self {
            listen,
            upstream: "https://registry.npmjs.org".to_string(),
            public_url,
            cache_dir,
            packument_ttl: Duration::from_secs(5 * 60),
        }
    }
}
