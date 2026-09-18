//! Which ecosystem's packages a `registries` entry serves, and the index
//! lists that follow from it.
//!
//! npm is absent from those lists: its registries are addressed by the scope
//! and prefix routes in [`super::RegistryLookups`], while every other
//! ecosystem resolves from an ordered list of indexes.

use super::{
    LoadWorkspaceYamlError, RegistryDeclaration, normalize_registry_url, quote_and_join,
    redact_registry_url,
};
use serde::Deserialize;
use std::{collections::BTreeMap, fmt};

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
/// that span entries are checked once the whole map has been read: an
/// ecosystem resolves from one index first, and pnpm resolves Cargo
/// dependencies from a single sparse index.
#[derive(Default)]
pub(super) struct DeclaredIndexes {
    urls: BTreeMap<Ecosystem, Vec<String>>,
    defaults: BTreeMap<Ecosystem, String>,
}

impl DeclaredIndexes {
    pub(super) fn add(
        &mut self,
        registry: &str,
        declaration: &RegistryDeclaration,
    ) -> Result<(), LoadWorkspaceYamlError> {
        let ecosystem = declaration.ecosystem();
        if ecosystem == Ecosystem::Npm {
            return match declaration.default {
                Some(_) => Err(LoadWorkspaceYamlError::NpmRegistryDeclaresDefault {
                    registry: redact_registry_url(registry),
                }),
                None => Ok(()),
            };
        }
        if let Some(field) = npm_only_field(declaration) {
            return Err(LoadWorkspaceYamlError::EcosystemRegistryDeclaresNpmField {
                registry: redact_registry_url(registry),
                ecosystem: ecosystem.to_string(),
                field: field.to_owned(),
            });
        }
        let normalized = normalize_registry_url(registry);
        self.urls
            .entry(ecosystem)
            .or_default()
            .push(normalized.clone());
        if declaration.is_default()
            && let Some(other) = self.defaults.insert(ecosystem, normalized.clone())
        {
            return Err(LoadWorkspaceYamlError::EcosystemDefaultDeclaredTwice {
                ecosystem: ecosystem.to_string(),
                registries: quote_and_join([other.as_str(), normalized.as_str()]),
            });
        }
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), LoadWorkspaceYamlError> {
        for (&ecosystem, urls) in &self.urls {
            if urls.len() < 2 {
                continue;
            }
            if ecosystem == Ecosystem::Cargo {
                return Err(LoadWorkspaceYamlError::CargoIndexDeclaredTwice {
                    registries: quote_and_join(urls.iter().map(String::as_str)),
                });
            }
            if !self.defaults.contains_key(&ecosystem) {
                return Err(LoadWorkspaceYamlError::EcosystemDefaultNotDeclared {
                    ecosystem: ecosystem.to_string(),
                    count: urls.len(),
                });
            }
        }
        Ok(())
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
/// The default index goes to the head, because that is the order the
/// ecosystem searches them in and a map keyed by URL carries no order of its
/// own.
pub(super) fn collect_index(
    indexes_by_ecosystem: &mut BTreeMap<Ecosystem, Vec<String>>,
    normalized: &str,
    declaration: &RegistryDeclaration,
) -> bool {
    let ecosystem = declaration.ecosystem();
    if ecosystem == Ecosystem::Npm {
        return false;
    }
    let indexes = indexes_by_ecosystem.entry(ecosystem).or_default();
    if declaration.is_default() {
        indexes.insert(0, normalized.to_owned());
    } else {
        indexes.push(normalized.to_owned());
    }
    true
}

/// Declare each ecosystem's indexes back into the `registries` shape.
pub(super) fn extend_with_indexes(
    declarations: &mut BTreeMap<String, RegistryDeclaration>,
    indexes_by_ecosystem: &BTreeMap<Ecosystem, Vec<String>>,
) {
    for (&ecosystem, indexes) in indexes_by_ecosystem {
        // A lone index is the one its ecosystem resolves from whether or not
        // the map says so, so `default` is written only where it
        // distinguishes this index from another.
        let names_a_default = indexes.len() > 1;
        for (position, index) in indexes.iter().enumerate() {
            let declaration = declarations.entry(index.clone()).or_default();
            declaration.ecosystem = Some(ecosystem);
            if position == 0 && names_a_default {
                declaration.default = Some(true);
            }
        }
    }
}
