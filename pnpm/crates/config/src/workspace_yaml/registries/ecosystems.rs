//! Which ecosystem's packages a `registries` entry serves, and the index
//! lists that follow from it.
//!
//! npm is absent from those lists: its registries are addressed by the scope
//! and prefix routes in [`super::RegistryLookups`], while every other
//! ecosystem resolves from an ordered list of indexes.
//!
//! A list is held in the order the configuration declares it, which is the
//! order the ecosystem searches: the last index answers what none before it
//! had.

use super::{
    LoadWorkspaceYamlError, RegistryDeclaration, RegistryLookups, normalize_registry_url,
    quote_and_join, redact_registry_url,
};
use indexmap::IndexMap;
use pnpm_lockfile::RegistryOptions;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// The package ecosystem whose packages a registry serves.
///
/// Closed, and narrower than the set of surfaces a pnpr server can host:
/// pnpm installs from these three, so an ecosystem it cannot install from
/// has no meaning in a `registries` entry and is refused where the entry is
/// parsed rather than somewhere further in.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    #[default]
    Npm,
    Cargo,
    Pypi,
}

impl Ecosystem {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Ecosystem::Npm => "npm",
            Ecosystem::Cargo => "cargo",
            Ecosystem::Pypi => "pypi",
        }
    }
}

impl fmt::Display for Ecosystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What the map has declared for each ecosystem other than npm, so the rules
/// that span entries are checked once the whole map has been read: one index
/// is declared once, and pnpm resolves Cargo dependencies from a single
/// sparse index.
#[derive(Default)]
pub(super) struct DeclaredIndexes {
    urls: BTreeMap<Ecosystem, Vec<String>>,
}

impl DeclaredIndexes {
    pub(super) fn add(
        &mut self,
        registry: &str,
        declaration: &RegistryDeclaration,
    ) -> Result<(), LoadWorkspaceYamlError> {
        let ecosystem = declaration.ecosystem();
        if ecosystem == Ecosystem::Npm {
            return Ok(());
        }
        if let Some(field) = npm_only_field(declaration) {
            return Err(LoadWorkspaceYamlError::EcosystemRegistryDeclaresNpmField {
                registry: redact_registry_url(registry),
                ecosystem: ecosystem.to_string(),
                field: field.to_owned(),
            });
        }
        // Keyed by URL, the map cannot tell that `.../simple` and
        // `.../simple/` are one index, so two spellings would take two places
        // in the search order and the second would never be reached.
        let normalized = normalize_registry_url(registry);
        let declared = self.urls.entry(ecosystem).or_default();
        if declared.contains(&normalized) {
            return Err(LoadWorkspaceYamlError::EcosystemIndexDeclaredTwice {
                ecosystem: ecosystem.to_string(),
                registry: redact_registry_url(&normalized),
            });
        }
        declared.push(normalized);
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), LoadWorkspaceYamlError> {
        match self.urls.get(&Ecosystem::Cargo) {
            Some(urls) if urls.len() > 1 => Err(LoadWorkspaceYamlError::CargoIndexDeclaredTwice {
                registries: quote_and_join(urls.iter().map(String::as_str)),
            }),
            _ => Ok(()),
        }
    }
}

/// The field this declaration sets that only an npm registry has: the two
/// that route npm packages, and the two that describe an npm server.
fn npm_only_field(declaration: &RegistryDeclaration) -> Option<&'static str> {
    [
        ("scopes", declaration.scopes.is_some()),
        ("prefix", declaration.prefix.is_some()),
        ("serverType", declaration.server_type.is_some()),
        ("supportsTimeField", declaration.supports_time_field.is_some()),
    ]
    .into_iter()
    .find_map(|(field, is_set)| is_set.then_some(field))
}

/// Record a non-npm entry's index, answering whether it was one.
///
/// Appended, so the list keeps the order the configuration declares, which is
/// the order the ecosystem searches.
pub(super) fn collect_index(
    indexes_by_ecosystem: &mut BTreeMap<Ecosystem, Vec<String>>,
    normalized: &str,
    declaration: &RegistryDeclaration,
) -> bool {
    let ecosystem = declaration.ecosystem();
    if ecosystem == Ecosystem::Npm {
        return false;
    }
    indexes_by_ecosystem
        .entry(ecosystem)
        .or_default()
        .push(normalized.to_owned());
    true
}

/// Declare each ecosystem's indexes back into the `registries` shape.
///
/// The map they are written into preserves insertion order, so reading the
/// result back declares the same search order.
pub(super) fn extend_with_indexes(
    declarations: &mut IndexMap<String, RegistryDeclaration>,
    indexes_by_ecosystem: &BTreeMap<Ecosystem, Vec<String>>,
) {
    for (&ecosystem, indexes) in indexes_by_ecosystem {
        for index in indexes {
            declarations.entry(index.clone()).or_default().ecosystem = Some(ecosystem);
        }
    }
}

/// Give every URL this layer declares the role this layer gives it, taking it
/// from whatever role an earlier layer had given it.
///
/// Each layer's `registries` is validated on its own, so nothing stops the
/// machine and the repository from disagreeing about what one URL serves.
/// Merging the lookups field by field would then leave the URL in both roles
/// at once: `cargo_index_url` and `python_indexes` could answer with the same
/// URL, `extend_with_indexes` would fold two ecosystems into one declaration,
/// and an npm route declared later would be deleted as though it were stale.
///
/// Applied before the layer's own lookups are merged in, so only the earlier
/// roles are dropped.
pub fn take_roles_from_earlier_layers(
    registries_by_scope: &mut BTreeMap<String, String>,
    registries_by_prefix: &mut BTreeMap<String, String>,
    registry_options_by_url: &mut BTreeMap<String, RegistryOptions>,
    indexes_by_ecosystem: &mut BTreeMap<Ecosystem, Vec<String>>,
    layer: &RegistryLookups,
) {
    let declared_as_index: BTreeSet<&str> = layer.indexes_by_ecosystem
        .values()
        .flatten()
        .map(String::as_str)
        .collect();
    let declared_at_all: BTreeSet<String> = declared_as_index
        .iter()
        .map(|registry| (*registry).to_owned())
        .chain(layer.registries_by_scope.values().cloned())
        .chain(layer.registries_by_prefix.values().map(|registry| normalize_registry_url(registry)))
        .chain(layer.registry_options_by_url.keys().cloned())
        .chain(layer.default_registry.clone())
        .collect();
    if declared_at_all.is_empty() {
        return;
    }
    // Whatever this layer says a URL is, it is no longer an index of some
    // other ecosystem, nor of the same one in another position.
    for indexes in indexes_by_ecosystem.values_mut() {
        indexes.retain(|registry| !declared_at_all.contains(registry.as_str()));
    }
    indexes_by_ecosystem.retain(|_, indexes| !indexes.is_empty());
    if declared_as_index.is_empty() {
        return;
    }
    // A URL this layer serves to another ecosystem routes no npm packages.
    registries_by_scope.retain(|_, registry| !declared_as_index.contains(registry.as_str()));
    registries_by_prefix.retain(|_, registry| {
        !declared_as_index.contains(normalize_registry_url(registry).as_str())
    });
    registry_options_by_url.retain(|registry, _| !declared_as_index.contains(registry.as_str()));
}
