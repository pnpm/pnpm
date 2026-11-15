//! Registry graphs stored in a test's database, the way
//! `pnpr registries import` stores one, since pnpr reads its registries only
//! from there.

use pnpr_config::{Config, RegistryGraphDocument, RoutingConfig, RoutingOptions, StorageConfig};
use pnpr_storage::{
    Storage,
    database::{HostedNamespace, NewRegistryGraph, RegistryGraphWrite},
};
use serde_json::{Map, Value};
use std::{net::SocketAddr, path::Path};

/// The registry-mock registries `pnpr/.fixtures/registries.yaml` declares.
pub const MOCK_REGISTRIES_YAML: &str = include_str!("../../../.fixtures/registries.yaml");

/// Store the registries document `yaml` as the next revision in the metadata
/// database of `config`, and serve it from `config.routing` too.
///
/// # Panics
///
/// The document does not parse, does not build, or cannot be stored.
pub fn seed_registries(config: &mut Config, yaml: &str) {
    seed_document(config, &parse_document(yaml));
}

/// The routing the registries document `yaml` builds under `config`, without
/// storing it, for a config whose database a test cannot reach.
///
/// # Panics
///
/// The document does not parse or does not build.
#[must_use]
pub fn registries_routing(config: &Config, yaml: &str) -> RoutingConfig {
    build_routing(config, &parse_document(yaml))
}

fn parse_document(yaml: &str) -> RegistryGraphDocument {
    RegistryGraphDocument::from_yaml(yaml)
        .unwrap_or_else(|error| panic!("parse the registries document: {error}"))
}

/// Load the config file at `path`, written with `registries`,
/// `defaultRegistry`, and `routes.public` in it: those keys move into the
/// database as its next revision, the empty graph when the file
/// declares none, and the file keeps the rest.
///
/// # Panics
///
/// The file cannot be read, rewritten, or loaded, or its registries cannot be
/// stored.
pub fn load_seeded_config(path: &Path, listen: SocketAddr, public_url: Option<String>) -> Config {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let (file, graph) = split_graph(&raw);
    std::fs::write(path, serde_json::to_string(&file).expect("serialize the config file"))
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    let mut config = Config::from_yaml(path, listen, public_url)
        .unwrap_or_else(|error| panic!("load {}: {error}", path.display()));
    let document: RegistryGraphDocument = graph
        .map(serde_json::from_value)
        .transpose()
        .unwrap_or_else(|error| panic!("parse the registries of {}: {error}", path.display()))
        .unwrap_or_default();
    seed_document(&mut config, &document);
    config
}

/// `yaml`, a config file, without its registry graph keys, and the graph
/// document they make up, if it declares any.
///
/// # Panics
///
/// `yaml` is not a mapping.
#[must_use]
pub fn split_graph(yaml: &str) -> (Map<String, Value>, Option<Value>) {
    let Value::Object(mut file) = serde_saphyr::from_str::<Value>(yaml)
        .unwrap_or_else(|error| panic!("parse the config file: {error}"))
    else {
        panic!("a config file is a mapping");
    };
    let mut graph = Map::new();
    for key in ["registries", "defaultRegistry"] {
        if let Some(value) = file.remove(key) {
            graph.insert(key.to_string(), value);
        }
    }
    if let Some(Value::Object(routes)) = file.get_mut("routes")
        && let Some(public) = routes.remove("public")
    {
        graph.insert("routes".to_string(), serde_json::json!({ "public": public }));
        if routes.is_empty() {
            file.remove("routes");
        }
    }
    (file, (!graph.is_empty()).then_some(Value::Object(graph)))
}

fn seed_document(config: &mut Config, document: &RegistryGraphDocument) {
    let routing = build_routing(config, document);
    store_document(&config.storage, document);
    config.routing = routing;
}

fn build_routing(config: &Config, document: &RegistryGraphDocument) -> RoutingConfig {
    let options = RoutingOptions {
        registry_env: &config.registry_env,
        allowed_private_networks: config.routing.route_policy
            .allowed_private_networks
            .clone(),
        resolve_upstreams: config.features.registry.enabled,
    };
    RoutingConfig::from_document(document, &options)
        .unwrap_or_else(|error| panic!("build the registries document: {error}"))
}

/// Store `document` as the next revision in the database of
/// `storage`.
///
/// # Panics
///
/// The storage cannot be opened or the revision cannot be written.
pub fn store_document(storage: &StorageConfig, document: &RegistryGraphDocument) {
    let storage = Storage::from_config(storage).expect("open the storage");
    store_into(&storage, document);
}

pub(crate) fn store_into(storage: &Storage, document: &RegistryGraphDocument) {
    crate::database::run(write_into(storage, document));
}

/// [`store_document`] on the caller's runtime, for a database served by that
/// runtime.
///
/// # Panics
///
/// The storage cannot be opened or the revision cannot be written.
pub async fn write_document(storage: &StorageConfig, document: &RegistryGraphDocument) {
    let storage = Storage::from_config(storage).expect("open the storage");
    write_into(&storage, document).await;
}

async fn write_into(storage: &Storage, document: &RegistryGraphDocument) {
    let bound: Vec<HostedNamespace> = document
        .registry_names()
        .filter_map(|name| document.hosted_storage(name))
        .map(|hosted| HostedNamespace { namespace: hosted.namespace, ecosystem: hosted.ecosystem })
        .collect();
    let json = serde_json::to_string(document).expect("serialize the registries document");
    let current =
        storage.read_registry_graph_generation().await.expect("read the stored registries version");
    let revision = NewRegistryGraph {
        document: &json,
        actor_id: "-",
        restored_from: None,
        vacated: &[],
        bound: &bound,
    };
    let written =
        storage.write_registry_graph(current, revision).await.expect("store the registries");
    assert!(matches!(written, RegistryGraphWrite::Written(_)), "{written:?}");
}
