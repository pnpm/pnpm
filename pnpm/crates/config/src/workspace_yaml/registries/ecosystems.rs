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
    LoadWorkspaceYamlError, RegistryDeclaration, normalize_registry_url, quote_and_join,
    redact_registry_url,
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

/// Forget the npm routes of a URL a later layer gave to another ecosystem.
///
/// Each layer is validated on its own, so a repository may serve a URL to
/// `PyPI` that the machine had routed npm scopes to. Merging those field by
/// field would leave one URL in both roles, and rebuilding the declarations
/// would then produce a `registries` entry that no layer could have written.
/// The layer that reclassified the URL wins, as it does for every other
/// setting.
pub fn drop_stale_roles(
    registries_by_scope: &mut BTreeMap<String, String>,
    registries_by_prefix: &mut BTreeMap<String, String>,
    registry_options_by_url: &mut BTreeMap<String, RegistryOptions>,
    indexes_by_ecosystem: &BTreeMap<Ecosystem, Vec<String>>,
) {
    let indexes: BTreeSet<&str> = indexes_by_ecosystem
        .values()
        .flatten()
        .map(String::as_str)
        .collect();
    if indexes.is_empty() {
        return;
    }
    registries_by_scope.retain(|_, registry| !indexes.contains(registry.as_str()));
    registries_by_prefix.retain(|_, registry| {
        !indexes.contains(normalize_registry_url(registry).as_str())
    });
    registry_options_by_url.retain(|registry, _| !indexes.contains(registry.as_str()));
}
