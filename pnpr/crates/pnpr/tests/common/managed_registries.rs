//! Configs whose registries the metadata database stores
//! (`registriesManagedBy: api`), seeded with a stored graph the way
//! `pnpr registries import` seeds one.

use pnpr::Config;
use pnpr_config::RegistryGraphDocument;
use pnpr_storage::{
    Storage,
    metadata::{NewRegistryGraph, RegistryGraphWrite},
};
use serde_json::{Map, Value};
use std::{net::SocketAddr, path::Path};

/// Load `yaml`, written like a pnpr.yaml that declares its registries, with
/// the registries moved into the metadata database: its `registries`,
/// `defaultRegistry`, and `routes.public` become the stored graph, and the
/// rest the config file at `path`, with `registriesManagedBy: api`.
pub async fn managed_config(
    path: &Path,
    yaml: &str,
    listen: SocketAddr,
    public_url: Option<String>,
) -> Config {
    let (file, graph) = split_graph(yaml);
    std::fs::write(path, serde_json::to_string(&file).unwrap()).unwrap();
    let config = Config::from_yaml(path, listen, public_url).unwrap();
    store_graph(&config, &graph).await;
    config
}

/// Store `graph`, the JSON shape of a registry graph document, as the next
/// revision.
pub async fn store_graph(config: &Config, graph: &Value) {
    let document: RegistryGraphDocument = serde_json::from_value(graph.clone()).unwrap();
    let document = serde_json::to_string(&document).unwrap();
    let storage = Storage::from_config(&config.storage).unwrap();
    let current = storage.read_registry_graph_generation().await.unwrap();
    let revision =
        NewRegistryGraph { document: &document, actor_id: "-", restored_from: None, vacated: &[] };
    let written = storage.write_registry_graph(current, revision).await.unwrap();
    assert!(matches!(written, RegistryGraphWrite::Written(_)), "{written:?}");
}

/// `yaml` as a config file with `registriesManagedBy: api`, and the graph it
/// declares.
pub fn split_graph(yaml: &str) -> (Map<String, Value>, Value) {
    let Value::Object(mut file) = serde_saphyr::from_str::<Value>(yaml).unwrap() else {
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
    file.insert("registriesManagedBy".to_string(), Value::String("api".to_string()));
    (file, Value::Object(graph))
}
