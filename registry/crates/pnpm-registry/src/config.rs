use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// Runtime configuration for the pnpm registry server.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub listen: SocketAddr,
    /// Upstream registry to proxy and cache from. `None` puts the
    /// server in *static* mode: cache misses become `404`, and the
    /// storage directory is treated as the authoritative source.
    pub upstream: Option<String>,
    /// URL clients should use to reach this server. Used to rewrite
    /// `dist.tarball` URLs in served packuments so tarball requests
    /// flow through this server.
    pub public_url: String,
    /// Directory under which packuments and tarballs live. The layout
    /// is Verdaccio's:
    ///
    /// ```text
    /// <storage>/<pkg>/package.json
    /// <storage>/<pkg>/<tarball-basename>.tgz
    /// ```
    ///
    /// In proxy mode this doubles as the cache; in static mode it's
    /// the source of truth.
    pub storage: PathBuf,
    /// How long a cached packument is considered fresh before it is
    /// re-fetched from the upstream. Ignored in static mode.
    pub packument_ttl: Duration,
}

impl Config {
    /// Build a proxy-mode config with the default npm upstream.
    pub fn proxy(listen: SocketAddr, storage: PathBuf) -> Self {
        let public_url = format!("http://{listen}");
        Self {
            listen,
            upstream: Some("https://registry.npmjs.org".to_string()),
            public_url,
            storage,
            packument_ttl: Duration::from_secs(5 * 60),
        }
    }

    /// Build a static-mode config that serves `storage` verbatim,
    /// never reaching out to a remote.
    pub fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self {
        let public_url = format!("http://{listen}");
        Self {
            listen,
            upstream: None,
            public_url,
            storage,
            packument_ttl: Duration::from_secs(5 * 60),
        }
    }
}
