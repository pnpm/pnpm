use indexmap::IndexMap;
use pnpr_config::{
    ArtifactsFeature, AuthConfig, BackendConfig, Config, CorsConfig, HostedConfig,
    HostedStoreConfig, LogConfig, OciConfig, OsvConfig, PipelineFeature, RegistryFeature,
    ResolverFeature, RoutePolicy, TrustedProxyHeader, UiConfig, UpstreamConfig, default_cache_dir,
    random_secret,
};
use pnpr_policy::{AccessList, PackageRule, PackageRules};
use pnpr_registry::{Ecosystem, PackagePattern, Registries, Registry};
use reqwest::header::HeaderMap;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

/// The namespace [`MockConfigPresets::proxy`] declares on its flat-root hosted org, so
/// those names resolve locally rather than to the npm upstream: the
/// registry-mock fixture scopes plus the unscoped fixtures. Kept in sync
/// with the fixtures under `pnpr/.fixtures/packages`.
pub const REGISTRY_MOCK_LOCAL_PATTERNS: &[&str] = &[
    "@foo/*",
    "@having/*",
    "@jsr/*",
    "@pnpm.e2e/*",
    "@private/*",
    "@scoped/*",
    "@pnpm/plugin-pnpmfile",
    "@pnpm/postinstall-modifies-source",
    "@pnpm/x",
    "@pnpm/xyz",
    "@pnpm/xyz-parent",
    "@pnpm/xyz-parent-parent",
    "@pnpm/xyz-parent-parent-parent",
    "@pnpm/xyz-parent-parent-parent-parent",
    "@pnpm/xyz-parent-parent-with-xyz",
    "@pnpm/y",
    "@pnpm/z",
    "@types/pnpm.e2e__peer-a",
    "@zkochan/test-pnpm-issue219",
    "ajv",
    "ajv-keywords",
    "create-touch-file-one-bin",
];

/// The `local` hosted registry's `packages:` rules in the registry-mock shape:
/// the fixture namespace ([`REGISTRY_MOCK_LOCAL_PATTERNS`]) with default rules,
/// `@private/*` and `@pnpm.e2e/needs-auth` restricted to authenticated callers,
/// and unpublish open to any authenticated user.
#[must_use]
pub fn registry_mock_rules() -> PackageRules {
    let authenticated = || Some(AccessList::from_tokens(["$authenticated"]));
    let mut rules: Vec<PackageRule> = REGISTRY_MOCK_LOCAL_PATTERNS
        .iter()
        .map(|pattern| PackageRule {
            pattern: PackagePattern::parse(pattern, Ecosystem::Npm)
                .expect("valid built-in fixture registry pattern"),
            access: (*pattern == "@private/*").then(authenticated).flatten(),
            publish: (*pattern == "@private/*").then(authenticated).flatten(),
            unpublish: (*pattern == "@private/*").then(authenticated).flatten(),
        })
        .collect();
    rules.push(PackageRule {
        pattern: PackagePattern::parse("@pnpm.e2e/needs-auth", Ecosystem::Npm)
            .expect("valid built-in fixture registry pattern"),
        access: authenticated(),
        publish: authenticated(),
        unpublish: authenticated(),
    });
    PackageRules::new(rules, None)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]))
}

#[must_use]
pub fn registry_mock_graph() -> (IndexMap<String, HostedConfig>, Registries) {
    registry_mock_graph_with(Vec::new())
}

fn registry_mock_graph_with(
    extra: Vec<(String, Registry)>,
) -> (IndexMap<String, HostedConfig>, Registries) {
    let rules = registry_mock_rules();
    let local_patterns = rules.patterns();
    let mut hosted = IndexMap::new();
    hosted.insert("local".to_string(), HostedConfig::new(String::new(), rules));
    let graph = [
        ("local".to_string(), Registry::Hosted { patterns: local_patterns }),
        ("npmjs".to_string(), Registry::Upstream { patterns: Vec::new() }),
        ("main".to_string(), Registry::Router { sources: registry_mock_sources() }),
    ];
    let registries =
        Registries::new(graph.into_iter().chain(extra).collect(), Some("main".to_string()));
    (hosted, registries)
}

fn registry_mock_sources() -> Vec<String> {
    vec!["local".to_string(), "npmjs".to_string()]
}

/// The registry key [`with_tar_registry`] adds.
pub const TAR_REGISTRY: &str = "zstd";

/// Add a [`TAR_REGISTRY`] router over the registry-mock sources that declares
/// `tarballs: tar`, so `/~zstd/` serves the fixtures as tars.
pub fn with_tar_registry(config: &mut Config) {
    let (_, registries) = registry_mock_graph_with(vec![(
        TAR_REGISTRY.to_string(),
        Registry::Router { sources: registry_mock_sources() },
    )]);
    config.routing.registries = registries;
    config.routing.tar_registries.insert(TAR_REGISTRY.to_string());
}

/// Convenience presets for constructing test configurations without a YAML file.
pub trait MockConfigPresets {
    /// Build a proxy-mode config in the registry-mock shape: fixture scopes are
    /// served locally from `storage`, while every other package proxies `npmjs`.
    fn proxy(listen: SocketAddr, storage: PathBuf) -> Self;

    /// Build a static-mode config that serves `storage` verbatim without upstreams.
    fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self;
}

impl MockConfigPresets for Config {
    fn proxy(listen: SocketAddr, storage: PathBuf) -> Self {
        proxy_config(listen, storage)
    }

    fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self {
        static_serve_config(listen, storage)
    }
}

/// Build a proxy-mode config in the registry-mock shape.
#[must_use]
pub fn proxy_config(listen: SocketAddr, storage: PathBuf) -> Config {
    let mut upstreams = IndexMap::new();
    upstreams.insert(
        "npmjs".to_string(),
        UpstreamConfig::with_defaults("https://registry.npmjs.org".to_string(), HeaderMap::new()),
    );
    let (hosted, registries) = registry_mock_graph();
    with_routing(
        listen,
        storage,
        pnpr_config::RoutingConfig {
            upstreams,
            route_policy: RoutePolicy::default(),
            registries,
            hosted,
            tar_registries: std::collections::BTreeSet::new(),
        },
    )
}

/// Build a static-mode config that serves `storage` verbatim.
#[must_use]
pub fn static_serve_config(listen: SocketAddr, storage: PathBuf) -> Config {
    let mut hosted = IndexMap::new();
    hosted.insert("local".to_string(), HostedConfig::new(String::new(), registry_mock_rules()));
    let graph = [
        ("local".to_string(), Registry::Hosted { patterns: Vec::new() }),
        ("main".to_string(), Registry::Router { sources: vec!["local".to_string()] }),
    ];
    let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    with_routing(
        listen,
        storage,
        pnpr_config::RoutingConfig {
            upstreams: IndexMap::new(),
            route_policy: RoutePolicy::default(),
            registries,
            hosted,
            tar_registries: std::collections::BTreeSet::new(),
        },
    )
}

fn with_routing(
    listen: SocketAddr,
    storage: PathBuf,
    routing: pnpr_config::RoutingConfig,
) -> Config {
    let cache_secret = random_secret();
    let oci_secret = pnpr_config::derive_subkey(&cache_secret, b"pnpr OCI token signing v1");
    Config {
        logs: LogConfig::default(),
        osv: OsvConfig::default(),
        resolution_cache_secret: Arc::clone(&cache_secret),
        cache_secret,
        oci_secret,
        has_configured_secret: false,
        http: pnpr_config::HttpConfig {
            listen,
            public_url: format!("http://{listen}"),
            cors: CorsConfig::default(),
            ui: UiConfig::default(),
            oci: OciConfig::default(),
            packument_ttl: Config::DEFAULT_PACKUMENT_TTL,
            trusted_proxies: Vec::new(),
            trusted_proxy_header: TrustedProxyHeader::default(),
            request_timeout: Some(pnpr_config::HttpConfig::DEFAULT_REQUEST_TIMEOUT),
        },
        storage: pnpr_config::StorageConfig {
            cache_dir: default_cache_dir(&storage),
            cache: pnpr_config::CacheConfig::default(),
            hosted_dir: storage,
            hosted_backend: HostedStoreConfig::Fs,
        },
        identity: pnpr_config::IdentityConfig {
            auth: AuthConfig::default(),
            backend: BackendConfig::Local,
            admins: std::collections::BTreeSet::new(),
        },
        features: pnpr_config::Features {
            registry: RegistryFeature::default(),
            resolver: ResolverFeature::default(),
            artifacts: ArtifactsFeature::default(),
            pipeline: PipelineFeature::default(),
        },
        routing,
    }
}
