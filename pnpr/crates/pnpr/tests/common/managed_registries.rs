//! Configs whose registries the metadata database stores, seeded with a
//! stored graph the way `pnpr registries import` seeds one.

use pnpr::Config;
use pnpr_config::RegistryGraphDocument;
use serde_json::Value;
use std::{net::SocketAddr, path::Path};

/// Load `yaml`, written like a pnpr.yaml that declares its registries, with
/// the registries moved into the metadata database: see
/// [`pnpr_fixtures::load_seeded_config`].
#[expect(clippy::unused_async, reason = "callers await it like the server setup around it")]
pub async fn managed_config(
    path: &Path,
    yaml: &str,
    listen: SocketAddr,
    public_url: Option<String>,
) -> Config {
    std::fs::write(path, yaml).unwrap();
    pnpr_fixtures::load_seeded_config(path, listen, public_url)
}

/// Store `graph`, the JSON shape of a registry graph document, as the next
/// revision, on the caller's runtime, which may be serving the database.
pub async fn store_graph(config: &Config, graph: &Value) {
    let document: RegistryGraphDocument = serde_json::from_value(graph.clone()).unwrap();
    pnpr_fixtures::write_document(&config.storage, &document).await;
}
