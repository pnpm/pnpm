use crate::Config;
use clap::{Args, builder::BoolishValueParser};
use std::{net::SocketAddr, path::PathBuf, time::Duration};

pub fn parse_cli_interval(raw: &str) -> std::result::Result<Duration, String> {
    pnpr_config::parse_interval(raw)
        .ok_or_else(|| format!("invalid interval {raw:?}; expected e.g. 30s, 5m, 1h, 1d"))
}

#[derive(Debug, Clone, Args)]
pub struct ServerArgs {
    /// Address to bind to.
    #[arg(long, default_value = Config::DEFAULT_LISTEN, env = "PNPR_LISTEN")]
    pub listen: SocketAddr,

    /// URL clients should use to reach this server. Used when
    /// rewriting `dist.tarball` URLs in served packuments. Defaults
    /// to `http://<listen>`.
    #[arg(long, env = "PNPR_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Duration before a cached packument is considered stale and
    /// refetched (e.g. "5m", "300s"). A registry's `maxage` overrides it.
    #[arg(long, env = "PNPR_PACKUMENT_TTL", alias = "packument-ttl-secs", value_parser = parse_cli_interval)]
    pub packument_ttl: Option<Duration>,

    #[command(flatten)]
    pub osv_options: OsvArgs,

    #[command(flatten)]
    pub features: FeatureArgs,
}

impl Default for ServerArgs {
    fn default() -> Self {
        Self {
            listen: Config::DEFAULT_LISTEN.parse().unwrap(),
            public_url: None,
            packument_ttl: None,
            osv_options: OsvArgs::default(),
            features: FeatureArgs::default(),
        }
    }
}

impl ServerArgs {
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        if other.listen.to_string() != Config::DEFAULT_LISTEN {
            self.listen = other.listen;
        }
        if other.public_url.is_some() {
            self.public_url = other.public_url;
        }
        if other.packument_ttl.is_some() {
            self.packument_ttl = other.packument_ttl;
        }
        if other.osv_options.osv.is_some() {
            self.osv_options.osv = other.osv_options.osv;
        }
        if other.osv_options.osv_db.is_some() {
            self.osv_options.osv_db = other.osv_options.osv_db;
        }
        if other.features.disable_registry {
            self.features.disable_registry = true;
        }
        if other.features.disable_resolver {
            self.features.disable_resolver = true;
        }
        if other.features.disable_artifacts {
            self.features.disable_artifacts = true;
        }
        if other.features.disable_pipeline {
            self.features.disable_pipeline = true;
        }
        self
    }
}

#[derive(Debug, Default, Clone, Args)]
pub struct OsvArgs {
    /// Enable local OSV npm vulnerability checks. Requires a local OSV
    /// npm database zip at `--osv-db` or `<cache>/osv/npm/all.zip`.
    #[arg(long, env = "PNPR_OSV", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub osv: Option<bool>,

    /// Path to the local OSV npm database zip or extracted JSON directory.
    #[arg(long, env = "PNPR_OSV_DB")]
    pub osv_db: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Args)]
pub struct FeatureArgs {
    /// Disable the npm-registry surface (packument/tarball reads, publish,
    /// unpublish, dist-tag, search) on this tier. Without the flag the
    /// surface is served whenever the loaded config declares at least one
    /// registry under `registries:`.
    #[arg(long, env = "PNPR_DISABLE_REGISTRY", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_registry: bool,

    /// Disable the install-accelerator surface (`/-/pnpr`, `/-/pnpr/v0/resolve`,
    /// `/-/pnpr/v0/verify-lockfile`). Overrides `resolver.enabled` from the
    /// loaded config.
    #[arg(long, env = "PNPR_DISABLE_RESOLVER", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_resolver: bool,

    /// Disable the signed shared-artifact surface. Overrides
    /// `artifacts.enabled` from the loaded config.
    #[arg(long, env = "PNPR_DISABLE_ARTIFACTS", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_artifacts: bool,

    /// Disable the pipeline run-record surface (`/-/pnpr/v0/pipeline`). Overrides
    /// `pipeline.enabled` from the loaded config.
    #[arg(long, env = "PNPR_DISABLE_PIPELINE", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub disable_pipeline: bool,
}

impl FeatureArgs {
    #[must_use]
    pub fn feature_overrides(&self) -> pnpr_config::FeatureOverrides {
        pnpr_config::FeatureOverrides {
            disable_registry: self.disable_registry,
            disable_resolver: self.disable_resolver,
            disable_artifacts: self.disable_artifacts,
            disable_pipeline: self.disable_pipeline,
        }
    }
}
