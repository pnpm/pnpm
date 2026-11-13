//! The registry graph a server stores on first start, when the metadata
//! database holds none.

use super::super::RegistryError;
use crate::audit::Actor;
use pnpr_config::{DEFAULT_REGISTRIES_YAML, RegistryGraphDocument};
use pnpr_storage::{
    Storage,
    metadata::{NewRegistryGraph, RegistryGraphWrite},
};

/// Store the bundled default registries as the first revision, unless the
/// database holds a revision. Replicas starting together write it once: the
/// write expects no stored graph, so every other replica's write conflicts
/// and that replica serves the stored one.
pub(crate) async fn seed_default_graph(storage: &Storage) -> Result<(), RegistryError> {
    if storage.read_registry_graph_generation().await?.is_some() {
        return Ok(());
    }
    let document = default_graph()?;
    let bound = super::stored::bound_namespaces(&document);
    let document = serde_json::to_string(&document)?;
    let revision = NewRegistryGraph {
        document: &document,
        actor_id: Actor::NONE.id,
        restored_from: None,
        vacated: &[],
        bound: &bound,
    };
    if let RegistryGraphWrite::Written(generation) =
        storage.write_registry_graph(None, revision).await?
    {
        crate::audit!(
            Actor::NONE,
            "registries.seed",
            version = %super::graph_version(Some(generation)),
            "default registries stored",
        );
    }
    Ok(())
}

/// The bundled default registries.
fn default_graph() -> Result<RegistryGraphDocument, RegistryError> {
    RegistryGraphDocument::from_yaml(DEFAULT_REGISTRIES_YAML)
}
