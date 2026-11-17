//! The registry graph a server stores on first start, when the metadata
//! database holds none.

use super::super::RegistryError;
use crate::audit::Actor;
use pnpr_config::{DEFAULT_REGISTRIES_YAML, RegistryGraphDocument};
use pnpr_storage::{
    Storage,
    database::{NewRegistryGraph, RegistryGraphWrite},
};

/// Store the bundled default registries as the first revision, unless the
/// database holds a revision. Replicas starting together write it once: the
/// write expects no stored graph, so every other replica's write conflicts
/// and that replica serves the stored one.
/// The hosted registries of the default graph are for private packages:
/// their default grants let signed-in callers read, not everyone.
fn keep_hosted_registries_private(
    seeded: &mut [pnpr_storage::database::StoredGrant],
    document: &RegistryGraphDocument,
) {
    let hosted = seeded
        .iter_mut()
        .filter(|grant| {
            document.registry_kind(&grant.content.name) == Some(pnpr_config::RegistryKind::Hosted)
        });
    for grant in hosted {
        for (subject, _) in &mut grant.content.subjects {
            if subject == "$all" {
                *subject = "$authenticated".to_string();
            }
        }
    }
}

pub(crate) async fn seed_default_graph(storage: &Storage) -> Result<(), RegistryError> {
    if storage.read_registry_graph_generation().await?.is_some() {
        return Ok(());
    }
    let document = default_graph()?;
    let bound = super::stored::bound_namespaces(&document);
    let mut seeded =
        super::stored::default_grants(&document, &pnpr_config::RegistryGraphDocument::default());
    keep_hosted_registries_private(&mut seeded, &document);
    let document = serde_json::to_string(&document)?;
    let revision = NewRegistryGraph {
        document: &document,
        actor_id: Actor::NONE.id,
        restored_from: None,
        vacated: &[],
        bound: &bound,
        seeded: &seeded,
        removed: &[],
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

#[cfg(test)]
mod tests;
