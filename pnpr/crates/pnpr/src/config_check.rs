use miette::IntoDiagnostic as _;
use pnpr_config::{Config, ConfigSource, FeatureOverrides, HostedStoreConfig, MaxUsers};
use pnpr_registry::Registry;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct RegistrySummary {
    pub name: String,
    pub kind: String,
    pub details: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SurfaceSummary {
    pub registry: bool,
    pub resolver: bool,
    pub artifacts: bool,
}

#[derive(Debug)]
pub struct ConfigCheckReport {
    pub source_description: String,
    pub registries: Vec<RegistrySummary>,
    pub default_registry: Option<String>,
    pub storage_backend: String,
    pub hosted_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub auth_summary: String,
    pub surfaces: SurfaceSummary,
}

pub fn check_config(config_path: Option<&Path>) -> miette::Result<ConfigCheckReport> {
    if let Some(path) = config_path.filter(|item| !item.exists()) {
        let display_path = path.display();
        return Err(miette::miette!("config file '{display_path}' does not exist"));
    }

    let default_listen: SocketAddr = Config::DEFAULT_LISTEN.parse().into_diagnostic()?;
    let overrides = FeatureOverrides {
        disable_registry: false,
        disable_resolver: false,
        disable_artifacts: false,
    };
    let auto_path = Config::auto_config_path();
    let (config, source) = Config::resolve_with_overrides(
        config_path,
        auto_path.as_deref(),
        default_listen,
        None,
        overrides,
    )
    .map_err(|err| miette::miette!("{err}"))?;

    let source_description = match &source {
        ConfigSource::Cli(cli_path) => format!("--config {}", cli_path.display()),
        ConfigSource::DefaultPath(default_path) => {
            format!("default path {}", default_path.display())
        }
        ConfigSource::Bundled => "bundled default".to_string(),
    };

    Ok(build_report(&config, source_description))
}

fn summarize_registry(config: &Config, name: &str) -> RegistrySummary {
    let (kind, details) = match config.routing.registries.get(name) {
        Some(Registry::Hosted { patterns }) => {
            let pat_count = patterns.len();
            let org = config.routing.hosted
                .get(name)
                .map(|hosted_entry| format!("org: {:?}", hosted_entry.org))
                .unwrap_or_default();
            ("hosted".to_string(), format!("{pat_count} patterns, {org}"))
        }
        Some(Registry::Upstream { patterns }) => {
            let url = config.routing.upstreams
                .get(name)
                .map(|upstream| upstream.url.as_str())
                .unwrap_or("unknown");
            ("upstream".to_string(), format!("url: {url}, {} patterns", patterns.len()))
        }
        Some(Registry::Router { sources }) => {
            ("router".to_string(), format!("sources: [{}]", sources.join(", ")))
        }
        None => ("unknown".to_string(), String::new()),
    };
    RegistrySummary { name: name.to_string(), kind, details }
}

fn describe_storage_backend(config: &Config) -> String {
    match &config.storage.hosted_backend {
        HostedStoreConfig::Fs => {
            let display_path = config.storage.hosted_dir.display();
            format!("local filesystem ({display_path})")
        }
        HostedStoreConfig::S3(settings) => {
            format!("S3 (bucket: {}, prefix: {:?})", settings.bucket, settings.prefix)
        }
        HostedStoreConfig::ObjectStore { prefix, .. } => {
            format!("custom object store (prefix: {prefix:?})")
        }
    }
}

fn build_report(config: &Config, source_description: String) -> ConfigCheckReport {
    let registries = config.routing.registries
        .names()
        .map(|name| summarize_registry(config, name))
        .collect();

    let default_registry = config.routing.registries
        .default_for(pnpr_registry::Ecosystem::Npm)
        .map(ToString::to_string);

    ConfigCheckReport {
        source_description,
        registries,
        default_registry,
        storage_backend: describe_storage_backend(config),
        hosted_dir: config.storage.hosted_dir.clone(),
        cache_dir: config.storage.cache_dir.clone(),
        auth_summary: format_auth_summary(config),
        surfaces: SurfaceSummary {
            registry: config.features.registry.enabled,
            resolver: config.features.resolver.enabled,
            artifacts: config.features.artifacts.enabled,
        },
    }
}

fn format_auth_summary(config: &Config) -> String {
    let ht = &config.identity.auth.htpasswd;
    let path = ht.file
        .as_ref()
        .map_or("in-memory", |file_path| file_path.to_str().unwrap_or("htpasswd"));
    let max = match ht.max_users {
        MaxUsers::Unlimited => "unlimited".to_string(),
        MaxUsers::Limited(n) => format!("max {n}"),
        MaxUsers::Disabled => "disabled".to_string(),
    };
    format!("htpasswd ({path}, registration: {max})")
}

#[must_use]
pub fn format_report(report: &ConfigCheckReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("Configuration check: OK\n");
    let _ = writeln!(out, "  Source: {}", report.source_description);
    let _ = writeln!(out, "  Registries ({}):", report.registries.len());
    for reg in &report.registries {
        let _ = writeln!(out, "    - {} ({}, {})", reg.name, reg.kind, reg.details);
    }
    if let Some(def) = &report.default_registry {
        let _ = writeln!(out, "  Default registry: {def}");
    }
    let _ = writeln!(out, "  Storage: {}", report.storage_backend);
    let display_cache = report.cache_dir.display();
    let _ = writeln!(out, "  Cache: {display_cache}");
    let _ = writeln!(out, "  Auth: {}", report.auth_summary);
    let _ = writeln!(
        out,
        "  Surfaces: registry={}, resolver={}, artifacts={}",
        report.surfaces.registry, report.surfaces.resolver, report.surfaces.artifacts,
    );
    out
}
