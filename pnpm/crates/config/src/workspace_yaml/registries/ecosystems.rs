//! Ecosystem index declarations and exclusive Python package routing.

use super::{
    EcosystemIndex,
    LoadWorkspaceYamlError,
    RegistryDeclaration,
    RegistryLookups,
    normalize_registry_url,
    quote_and_join,
    redact_registry_url,
};
use indexmap::IndexMap;
use pnpm_lockfile::RegistryOptions;
use serde::Deserialize;
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
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
    urls: BTreeMap<Ecosystem, Vec<EcosystemIndex>>,
}

impl DeclaredIndexes {
    pub(super) fn add(
        &mut self,
        registry: &str,
        declaration: &RegistryDeclaration,
    ) -> Result<(), LoadWorkspaceYamlError> {
        let ecosystem = declaration.ecosystem();
        if ecosystem != Ecosystem::Pypi && declaration.packages.is_some() {
            return Err(LoadWorkspaceYamlError::InvalidPythonRegistryPackages {
                registry: redact_registry_url(registry),
                reason: "packages is only supported for pypi registries".to_string(),
            });
        }
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
        let normalized = normalize_registry_url(registry);
        let declared = self.urls.entry(ecosystem).or_default();
        if declared
            .iter()
            .any(|index| index.url == normalized)
        {
            return Err(LoadWorkspaceYamlError::EcosystemIndexDeclaredTwice {
                ecosystem: ecosystem.to_string(),
                registry: redact_registry_url(&normalized),
            });
        }
        declared.push(EcosystemIndex { url: normalized, packages: declaration.packages.clone() });
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), LoadWorkspaceYamlError> {
        match self.urls.get(&Ecosystem::Cargo) {
            Some(urls) if urls.len() > 1 => Err(LoadWorkspaceYamlError::CargoIndexDeclaredTwice {
                registries: quote_and_join(urls.iter().map(|index| index.url.as_str())),
            }),
            _ => super::python::validate_routes(
                self.urls
                    .get(&Ecosystem::Pypi)
                    .map_or(&[], Vec::as_slice),
            ),
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
pub(super) fn collect_index(
    indexes_by_ecosystem: &mut BTreeMap<Ecosystem, Vec<EcosystemIndex>>,
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
        .push(EcosystemIndex {
            url: normalized.to_owned(),
            packages: declaration.packages.clone(),
        });
    true
}

/// Declare each ecosystem's indexes and package routes back into `registries`.
pub(super) fn extend_with_indexes(
    declarations: &mut IndexMap<String, RegistryDeclaration>,
    indexes_by_ecosystem: &BTreeMap<Ecosystem, Vec<EcosystemIndex>>,
) {
    for (&ecosystem, indexes) in indexes_by_ecosystem {
        for index in indexes {
            let declaration = declarations.entry(index.url.clone()).or_default();
            declaration.ecosystem = Some(ecosystem);
            declaration.packages.clone_from(&index.packages);
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
    indexes_by_ecosystem: &mut BTreeMap<Ecosystem, Vec<EcosystemIndex>>,
    layer: &RegistryLookups,
) {
    let declared_as_index: BTreeSet<&str> = layer.indexes_by_ecosystem
        .values()
        .flatten()
        .map(|index| index.url.as_str())
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
    for indexes in indexes_by_ecosystem.values_mut() {
        indexes.retain(|registry| !declared_at_all.contains(registry.url.as_str()));
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

/// Whether `registry` is an index of an ecosystem other than npm, comparing
/// the normalized URL because a declaration's key is normalized and a
/// `namedRegistries` alias is kept as written.
#[must_use]
pub fn serves_another_ecosystem(
    indexes_by_ecosystem: &BTreeMap<Ecosystem, Vec<EcosystemIndex>>,
    registry: &str,
) -> bool {
    let normalized = normalize_registry_url(registry);
    indexes_by_ecosystem
        .values()
        .flatten()
        .any(|index| index.url == normalized)
}
