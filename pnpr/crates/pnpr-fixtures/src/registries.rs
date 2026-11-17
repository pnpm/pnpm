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

/// A registries document with the access rules a test wrote in it taken out.
pub(crate) struct Graph {
    document: RegistryGraphDocument,
    rules: crate::rules::Tables,
}

impl Graph {
    /// # Panics
    ///
    /// The document does not parse.
    pub(crate) fn of(mut graph: Value) -> Self {
        let rules = crate::rules::take_rules(&mut graph);
        let document = serde_json::from_value(graph)
            .unwrap_or_else(|error| panic!("parse the registries document: {error}"));
        Self { document, rules }
    }

    fn of_document(document: &RegistryGraphDocument) -> Self {
        Self::of(serde_json::to_value(document).expect("serialize the registries document"))
    }
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

pub(crate) fn parse_document(yaml: &str) -> Graph {
    let graph = serde_saphyr::from_str::<Option<Value>>(yaml)
        .unwrap_or_else(|error| panic!("parse the registries document: {error}"))
        .unwrap_or_else(|| Value::Object(Map::new()));
    Graph::of(graph)
}

/// Load the config file at `path`, written with `registries`,
/// `defaultRegistry`, and `routes.public` in it: those keys move into the
/// database as its next revision, the empty graph when the file
/// declares none, and the file keeps the rest. The registries may write
/// access rules the way a registry once declared them; the grants that give
/// the same are stored with the graph.
///
/// A top-level `teams` key also moves into the database. It maps each team ID
/// to `{ name, members }`, with `members` a list of account IDs.
///
/// # Panics
///
/// The file cannot be read, rewritten, or loaded, or its registries cannot be
/// stored.
pub fn load_seeded_config(path: &Path, listen: SocketAddr, public_url: Option<String>) -> Config {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let (mut file, graph) = split_graph(&raw);
    let teams = file.remove("teams");
    std::fs::write(path, serde_json::to_string(&file).expect("serialize the config file"))
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    let mut config = Config::from_yaml(path, listen, public_url)
        .unwrap_or_else(|error| panic!("load {}: {error}", path.display()));
    if let Some(Value::Object(teams)) = teams {
        let storage = Storage::from_config(&config.storage).expect("open the storage");
        crate::database::run(store_teams(&storage, &teams));
    }
    let graph = Graph::of(graph.unwrap_or_else(|| Value::Object(Map::new())));
    seed_document(&mut config, &graph);
    config
}

async fn store_teams(storage: &Storage, teams: &Map<String, Value>) {
    for (id, team) in teams {
        let name = team["name"].as_str().expect("a team has a name");
        storage.create_team(id, name).await.expect("store the team");
        for member in team["members"].as_array().expect("a team lists its members") {
            let member = member.as_str().expect("a member is an account ID");
            storage.add_team_member(id, member).await.expect("store the team member");
        }
    }
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

fn seed_document(config: &mut Config, graph: &Graph) {
    let routing = build_routing(config, graph);
    let storage = Storage::from_config(&config.storage).expect("open the storage");
    store_into(&storage, graph);
    config.routing = routing;
}

fn build_routing(config: &Config, graph: &Graph) -> RoutingConfig {
    let options = RoutingOptions {
        registry_env: &config.registry_env,
        allowed_private_networks: config.routing.route_policy
            .allowed_private_networks
            .clone(),
        resolve_upstreams: config.features.registry.enabled,
    };
    let routing = RoutingConfig::from_document(&graph.document, &options)
        .unwrap_or_else(|error| panic!("build the registries document: {error}"));
    crate::rules::apply(&routing, &graph.rules);
    routing
}

/// Store `document` as the next revision in the database of
/// `storage`.
///
/// # Panics
///
/// The storage cannot be opened or the revision cannot be written.
pub fn store_document(storage: &StorageConfig, document: &RegistryGraphDocument) {
    let storage = Storage::from_config(storage).expect("open the storage");
    store_into(&storage, &Graph::of_document(document));
}

pub(crate) fn store_into(storage: &Storage, graph: &Graph) {
    crate::database::run(write_into(storage, graph));
}

/// [`store_document`] on the caller's runtime, for a database served by that
/// runtime.
///
/// # Panics
///
/// The storage cannot be opened or the revision cannot be written.
pub async fn write_document(storage: &StorageConfig, document: &RegistryGraphDocument) {
    let storage = Storage::from_config(storage).expect("open the storage");
    write_into(&storage, &Graph::of_document(document)).await;
}

/// [`write_document`] for a document in its JSON shape, which may write
/// access rules the way [`load_seeded_config`] reads them.
///
/// # Panics
///
/// The document does not parse, or the storage cannot be opened or written.
pub async fn write_graph(storage: &StorageConfig, graph: &Value) {
    let storage = Storage::from_config(storage).expect("open the storage");
    write_into(&storage, &Graph::of(graph.clone())).await;
}

async fn write_into(storage: &Storage, graph: &Graph) {
    let document = &graph.document;
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
        seeded: &[],
        removed: &[],
    };
    let written =
        storage.write_registry_graph(current, revision).await.expect("store the registries");
    assert!(matches!(written, RegistryGraphWrite::Written(_)), "{written:?}");
    crate::rules::store(storage, &graph.rules).await;
}
