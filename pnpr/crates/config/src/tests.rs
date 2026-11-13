mod access;

mod storage;

mod file_loading;

mod behavior;

mod registry_graph;

mod upstream;

mod server_options;

mod graph_document;

use super::{
    ArtifactQuota, BackendConfig, Config, ConfigSource, DEFAULT_CONFIG_YAML,
    DEFAULT_REGISTRIES_YAML, FeatureOverrides, HostedStoreConfig, Interval, LogFormat, LogLevel,
    RegistryGraphDocument, RoutingConfig, RoutingOptions, S3Settings, TeamDirectory,
    UpstreamAuthFile, UpstreamConfig, UpstreamConfigFile, config_file_in, normalize_key_prefix,
    parse_interval, resolve_relative, resolve_upstream_config,
    upstream::{TokenEnv, UpstreamAuthType},
};
use indexmap::IndexMap;
use object_store::{ClientConfigKey, aws::AmazonS3ConfigKey};
use pnpm_env_replace::EnvVar;
use pnpm_testing_utils::env_guard::EnvGuard;
use pnpr_error::RegistryError;
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use reqwest::header::AUTHORIZATION;
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    time::Duration,
};

/// Test [`EnvVar`] provider with a fixed set of variables, so
/// `token_env` resolution can be exercised without touching the real
/// process environment.
struct FakeEnv;

impl EnvVar for FakeEnv {
    fn var(name: &str) -> Option<String> {
        match name {
            "NPM_TOKEN" => Some("default-env-token".to_string()),
            "CUSTOM_TOKEN" => Some("custom-env-token".to_string()),
            "EMPTY_TOKEN" => Some(String::new()),
            "BASIC_TOKEN" => Some("dXNlcjpwYXNz".to_string()),
            "ORG_NAME" => Some("acme".to_string()),
            _ => None,
        }
    }
}

/// Load `yaml` as a config file plus the registry graph its graph keys
/// declare (`registries`, `defaultRegistry`, `routes.public`), as a server
/// does with the graph stored in the database: the graph is held to
/// the stored-document rules under the file's `registryEnv`, its credentials
/// resolve through [`FakeEnv`], and it becomes `config.routing`.
fn load_with_graph(
    yaml: &str,
    base_dir: &Path,
    listen: SocketAddr,
    public_url: Option<String>,
) -> Result<Config, RegistryError> {
    load_with_graph_and_overrides(
        yaml,
        base_dir,
        Some(listen),
        public_url,
        FeatureOverrides::default(),
    )
}

/// [`load_with_graph`] with feature overrides.
fn load_with_graph_and_overrides(
    yaml: &str,
    base_dir: &Path,
    listen: Option<SocketAddr>,
    public_url: Option<String>,
    overrides: FeatureOverrides,
) -> Result<Config, RegistryError> {
    let (config_yaml, graph_yaml) = split_graph_keys(yaml);
    let mut config = Config::from_yaml_str_with_overrides(
        &config_yaml,
        base_dir,
        listen,
        public_url,
        overrides,
    )?;
    let document = RegistryGraphDocument::from_yaml(&graph_yaml)?;
    let options = RoutingOptions {
        registry_env: &config.registry_env,
        allowed_private_networks: config.routing.route_policy
            .allowed_private_networks
            .clone(),
        resolve_upstreams: !overrides.disable_registry,
    };
    config.routing = RoutingConfig::from_document_with_env::<FakeEnv>(&document, &options)?;
    Ok(config)
}

/// Split block-style `yaml` by line into its config-file part and its
/// registry graph part. A `routes:` block is split by child key: `public`
/// goes to the graph, the rest stays in the config file.
fn split_graph_keys(yaml: &str) -> (String, String) {
    let mut split = GraphSplit::default();
    for line in yaml.split_inclusive('\n') {
        split.push(line);
    }
    split.finish()
}

/// The parts [`split_graph_keys`] has collected, and the keys the next line
/// falls under.
#[derive(Default)]
struct GraphSplit {
    config: String,
    graph: String,
    config_routes: String,
    graph_routes: String,
    top_key: String,
    route_key: String,
    route_indent: Option<usize>,
}

impl GraphSplit {
    fn push(&mut self, line: &str) {
        let indent = line.len() - line.trim_start().len();
        if indent == 0 && starts_a_key(line) {
            self.top_key = yaml_key(line);
            self.route_key.clear();
            self.route_indent = None;
            if line.trim_end() == "routes:" {
                return;
            }
        } else if self.top_key == "routes"
            && starts_a_key(line)
            && self.route_indent.is_none_or(|child| child == indent)
        {
            self.route_indent = Some(indent);
            self.route_key = yaml_key(line);
        }
        let target = match (self.top_key.as_str(), self.route_key.as_str()) {
            ("registries" | "defaultRegistry", _) => &mut self.graph,
            ("routes", "public") => &mut self.graph_routes,
            ("routes", _) if indent > 0 => &mut self.config_routes,
            _ => &mut self.config,
        };
        target.push_str(line);
    }

    fn finish(mut self) -> (String, String) {
        append_routes(&mut self.config, &self.config_routes);
        append_routes(&mut self.graph, &self.graph_routes);
        (self.config, self.graph)
    }
}

fn starts_a_key(line: &str) -> bool {
    let content = line.trim_start();
    !content.is_empty() && !content.starts_with('#') && !content.starts_with('-')
}

fn yaml_key(line: &str) -> String {
    line.trim_start()
        .split_once(':')
        .map_or("", |(key, _)| key.trim())
        .to_string()
}

fn append_routes(part: &mut String, routes: &str) {
    if !routes.is_empty() {
        part.push_str("routes:\n");
        part.push_str(routes);
    }
}

/// The routing of the registries a server stores on first start.
fn bundled_registries() -> RoutingConfig {
    let document = RegistryGraphDocument::from_yaml(DEFAULT_REGISTRIES_YAML).unwrap();
    let options = RoutingOptions {
        registry_env: &[],
        allowed_private_networks: Vec::new(),
        resolve_upstreams: true,
    };
    RoutingConfig::from_document_with_env::<FakeEnv>(&document, &options).unwrap()
}

fn upstream_config_file(
    auth: Option<UpstreamAuthFile>,
    headers: IndexMap<String, String>,
) -> UpstreamConfigFile {
    UpstreamConfigFile {
        url: "https://upstream.test/".to_string(),
        auth,
        headers: headers
            .into_iter()
            .map(|(name, value)| (name, value.into()))
            .collect(),
        packument_ttl: None,
        timeout: None,
        max_packument_size: None,
        max_fails: None,
        fail_timeout: None,
        cache: None,
        search: false,
        access: None,
    }
}

fn auth_header(upstream: &super::UpstreamConfig) -> Option<&str> {
    upstream.headers
        .get(AUTHORIZATION)
        .map(|value| value.to_str().unwrap())
}

/// [`resolve_upstream_config`] with no declared teams, as every serving-knob
/// case here has.
fn resolve_upstream(name: &str, file: UpstreamConfigFile) -> Result<UpstreamConfig, RegistryError> {
    resolve_upstream_config::<FakeEnv>(name, file, &TeamDirectory::default())
}

fn user(name: &str) -> Identity {
    Identity::user(name)
}

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

// ----- LogFormat / LogLevel serde behavior ------------------------------

/// Helper: deserialize a YAML scalar into the requested enum.
/// Lets us assert the variant mapping concisely.
fn parse_log_yaml<Target: serde::de::DeserializeOwned>(yaml: &str) -> Result<Target, String> {
    serde_saphyr::from_str::<Target>(yaml).map_err(|err| err.to_string())
}

// ----- Config::resolve precedence ---------------------------------------

/// Helper: write a config file under a tempdir and hand back the
/// path. Tests use this to populate both the explicit `-c` arg
/// and the auto-discovered default path.
fn write_yaml(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write yaml fixture");
    path
}

const MINIMAL_YAML: &str = "storage: ./s\n";

// ----- per-registry `packages:` rules from YAML -------------------------

/// A one-hosted-registry config whose `packages:` map is the given YAML
/// fragment (indented under `packages:`).
fn hosted_rules_config(packages: &str) -> Config {
    let yaml = format!(
        "storage: ./s\nregistries:\n  local:\n    type: hosted\n    packages:\n{packages}",
    );
    load_with_graph(&yaml, Path::new("/x"), listen(), None).unwrap()
}

/// The same one-hosted-registry config as [`hosted_rules_config`], for
/// the `packages:` fragments that must fail to load.
fn hosted_rules_err(packages: &str) -> RegistryError {
    let yaml = format!(
        "storage: ./s\nregistries:\n  local:\n    type: hosted\n    packages:\n{packages}",
    );
    load_with_graph(&yaml, Path::new("/x"), listen(), None).unwrap_err()
}

fn s3_settings_for(endpoint: Option<&str>, allow_http: Option<bool>) -> S3Settings {
    S3Settings {
        bucket: "packages".to_string(),
        region: None,
        endpoint: endpoint.map(str::to_string),
        prefix: None,
        access_key_id: None,
        secret_access_key: None,
        force_path_style: None,
        allow_http,
    }
}
