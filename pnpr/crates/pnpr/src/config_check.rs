use miette::IntoDiagnostic as _;
use pnpr_config::{
    BackendConfig, Config, ConfigSource, FeatureOverrides, HostedStoreConfig, MaxUsers,
};
use pnpr_package_name::Ecosystem;
use pnpr_registry::{Registries, Registry};
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
    /// The default registry of each ecosystem that has one, as `(ecosystem, registry)`.
    pub default_registries: Vec<(Ecosystem, String)>,
    pub storage_backend: String,
    pub hosted_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub auth_summary: String,
    pub surfaces: SurfaceSummary,
}

#[must_use]
pub fn check_resolved_config(config: &Config, source: &ConfigSource) -> ConfigCheckReport {
    let source_description = match source {
        ConfigSource::Cli(cli_path) => format!("--config {}", cli_path.display()),
        ConfigSource::DefaultPath(default_path) => {
            format!("default path {}", default_path.display())
        }
        ConfigSource::Bundled => "bundled default".to_string(),
    };

    build_report(config, source_description)
}

pub fn check_config(config_path: Option<&Path>) -> miette::Result<ConfigCheckReport> {
    if let Some(path) = config_path.filter(|item| !item.exists()) {
        let display_path = path.display();
        return Err(miette::miette!("config file '{display_path}' does not exist"));
    }

    let default_listen: SocketAddr = Config::DEFAULT_LISTEN.parse().into_diagnostic()?;
    let overrides = FeatureOverrides::default();
    let auto_path = Config::auto_config_path();
    let (config, source) = Config::resolve_with_overrides(
        config_path,
        auto_path.as_deref(),
        default_listen,
        None,
        overrides,
    )
    .map_err(|err| miette::miette!("{err}"))?;

    Ok(check_resolved_config(&config, &source))
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
                .map(|upstream| sanitize_diagnostic_url(upstream.url.as_str()))
                .unwrap_or_else(|| "unknown".to_string());
            ("upstream".to_string(), format!("url: {url}, {} patterns", patterns.len()))
        }
        Some(Registry::Router { sources }) => {
            ("router".to_string(), format!("sources: [{}]", sources.join(", ")))
        }
        None => ("unknown".to_string(), String::new()),
    };
    RegistrySummary { name: name.to_string(), kind, details }
}

fn sanitize_diagnostic_url(raw: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(raw) else {
        return "<unparsable URL>".to_string();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_fragment(None);
    if parsed.query().is_some() {
        redact_sensitive_query(&mut parsed);
    }
    parsed.to_string()
}

fn redact_sensitive_query(parsed: &mut url::Url) {
    let pairs: Vec<_> = parsed
        .query_pairs()
        .map(|(k, v)| {
            if is_sensitive_param(&k) {
                (k.into_owned(), "REDACTED".to_string())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();
    parsed
        .query_pairs_mut()
        .clear()
        .extend_pairs(pairs);
}

fn is_sensitive_param(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("token")
        || lower.contains("secret")
        || lower.contains("key")
        || lower.contains("auth")
        || lower.contains("pass")
        || lower.contains("sig")
        || lower == "user"
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

    let default_registries = Ecosystem::all()
        .filter_map(|ecosystem| {
            let key = config.routing.registries.default_for(ecosystem)?;
            Some((ecosystem, Registries::local_name(key).to_string()))
        })
        .collect();

    ConfigCheckReport {
        source_description,
        registries,
        default_registries,
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
    let max = match ht.max_users {
        MaxUsers::Unlimited => "unlimited".to_string(),
        MaxUsers::Limited(n) => format!("max {n}"),
        MaxUsers::Disabled => "disabled".to_string(),
    };
    let (backend, location) = match &config.identity.backend {
        BackendConfig::Local => (
            "htpasswd",
            match &ht.file {
                Some(path) => path.display().to_string(),
                None => "in-memory".to_string(),
            },
        ),
        BackendConfig::Libsql(settings) => {
            let url = sanitize_diagnostic_url(&settings.url);
            let location = match &settings.replica_path {
                Some(replica) => format!("{url}, replica: {}", replica.display()),
                None => url,
            };
            ("libsql", location)
        }
        BackendConfig::Postgres(settings) => ("postgres", sanitize_diagnostic_url(&settings.url)),
        BackendConfig::Mysql(settings) => ("mysql", sanitize_diagnostic_url(&settings.url)),
    };
    format!("{backend} ({location}, registration: {max})")
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
    if !report.default_registries.is_empty() {
        let defaults: Vec<String> = report.default_registries
            .iter()
            .map(|(ecosystem, registry)| format!("{ecosystem}={registry}"))
            .collect();
        let _ = writeln!(out, "  Default registries: {}", defaults.join(", "));
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
