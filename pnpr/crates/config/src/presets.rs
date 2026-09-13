use super::{
    ArtifactsFeature, AuthConfig, BackendConfig, Config, CorsConfig, HeaderMap, HostedConfig,
    HostedStoreConfig, IndexMap, LogConfig, OciConfig, OsvConfig, PathBuf, PipelineFeature,
    Registries, Registry, RegistryFeature, ResolverFeature, RoutePolicy, SocketAddr, Teams,
    UpstreamConfig, default_cache_dir, random_secret, registry_mock_graph, registry_mock_rules,
};

impl Config {
    /// Build a proxy-mode config in the registry-mock shape: the fixture scopes
    /// (and the one unscoped fixture) are the declared namespace of a flat-root
    /// hosted org over `storage`, while every other name proxies to the
    /// pattern-less `npmjs` upstream. The path-less base aliases the `main`
    /// router. Kept for callers that don't use YAML config (notably pacquet's
    /// test registry, whose fixtures are served locally while real npm packages
    /// fall through to npmjs). The local pattern set mirrors the fixture
    /// subset of the bundled `config.yaml` `local` registry
    /// (`REGISTRY_MOCK_LOCAL_PATTERNS`); the YAML additionally claims the
    /// exact names the TS test suite publishes, which never reach this
    /// constructor.
    #[must_use]
    pub fn proxy(listen: SocketAddr, storage: PathBuf) -> Self {
        let mut upstreams = IndexMap::new();
        upstreams.insert(
            "npmjs".to_string(),
            UpstreamConfig::with_defaults(
                "https://registry.npmjs.org".to_string(),
                HeaderMap::new(),
            ),
        );
        let (hosted, registries) = registry_mock_graph();
        Self::with_routing(
            listen,
            storage,
            super::RoutingConfig {
                upstreams,
                route_policy: RoutePolicy::default(),
                registries,
                hosted,
            },
        )
    }

    /// Build a static-mode config that serves `storage` verbatim: one
    /// pattern-less hosted registry over the storage root (an empty `org`
    /// namespace == the flat root), the sole source of a router that the
    /// path-less base aliases. Every package resolves to that one hosted
    /// origin — no upstream, no fall-through.
    #[must_use]
    pub fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self {
        let mut hosted = IndexMap::new();
        // The graph entry below is pattern-less — static mode claims and
        // serves every name in `storage` — while the rules still carry the
        // registry-mock protections (`@private/*`, `@pnpm.e2e/needs-auth`,
        // authenticated unpublish). Programmatic configs may split the
        // namespace (graph) from the rules like this; YAML derives both from
        // one `packages:` map.
        hosted.insert(
            "local".to_string(),
            HostedConfig {
                org: String::new(),
                rules: registry_mock_rules(),
                teams: Teams::default(),
            },
        );
        let graph = [
            ("local".to_string(), Registry::Hosted { patterns: Vec::new() }),
            ("main".to_string(), Registry::Router { sources: vec!["local".to_string()] }),
        ];
        let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
        Self::with_routing(
            listen,
            storage,
            super::RoutingConfig {
                upstreams: IndexMap::new(),
                route_policy: RoutePolicy::default(),
                registries,
                hosted,
            },
        )
    }

    fn with_routing(listen: SocketAddr, storage: PathBuf, routing: super::RoutingConfig) -> Self {
        Self {
            logs: LogConfig::default(),
            osv: OsvConfig::default(),
            resolution_cache_secret: random_secret(),
            http: super::HttpConfig {
                listen,
                public_url: format!("http://{listen}"),
                cors: CorsConfig::default(),
                oci: OciConfig::default(),
                packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            },
            storage: super::StorageConfig {
                cache_dir: default_cache_dir(&storage),
                hosted_dir: storage,
                hosted_backend: HostedStoreConfig::Fs,
            },
            identity: super::IdentityConfig {
                auth: AuthConfig::default(),
                backend: BackendConfig::Local,
            },
            features: super::Features {
                registry: RegistryFeature::default(),
                resolver: ResolverFeature::default(),
                artifacts: ArtifactsFeature::default(),
                pipeline: PipelineFeature::default(),
            },
            routing,
        }
    }
}
