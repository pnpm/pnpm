mod access;

mod storage;

mod file_loading;

mod behavior;

mod registry_graph;

mod upstream;

mod server_options;

use super::{
    BackendConfig, Config, ConfigSource, DEFAULT_CONFIG_YAML, FeatureOverrides, HostedStoreConfig,
    Interval, LogFormat, LogLevel, S3Settings, Teams, UpstreamAuthFile, UpstreamConfig,
    UpstreamConfigFile, config_file_in, normalize_key_prefix, parse_interval, resolve_relative,
    resolve_upstream_config,
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
            _ => None,
        }
    }
}

fn upstream_config_file(
    auth: Option<UpstreamAuthFile>,
    headers: IndexMap<String, String>,
) -> UpstreamConfigFile {
    UpstreamConfigFile {
        url: "https://upstream.test/".to_string(),
        auth,
        headers,
        maxage: None,
        timeout: None,
        max_fails: None,
        fail_timeout: None,
        cache: None,
        search: false,
        access: None,
    }
}

fn auth_header(upstream: &super::UpstreamConfig) -> Option<&str> {
    upstream.headers.get(AUTHORIZATION).map(|value| value.to_str().unwrap())
}

/// [`resolve_upstream_config`] with no declared teams, as every serving-knob
/// case here has.
fn resolve_upstream(name: &str, file: UpstreamConfigFile) -> Result<UpstreamConfig, RegistryError> {
    resolve_upstream_config::<FakeEnv>(name, file, &Teams::default())
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
    Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None).unwrap()
}

/// The same one-hosted-registry config as [`hosted_rules_config`], for
/// the `packages:` fragments that must fail to load.
fn hosted_rules_err(packages: &str) -> RegistryError {
    let yaml = format!(
        "storage: ./s\nregistries:\n  local:\n    type: hosted\n    packages:\n{packages}",
    );
    Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None).unwrap_err()
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
