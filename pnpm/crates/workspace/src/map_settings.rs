//! Package maps with an optional policy for unmatched entries.

pub mod specifiers;

use serde::{
    Deserialize,
    Serialize,
};

/// How `pnpm add` / `pnpm update` reconcile a directly-specified version
/// against a `catalog:` entry for the same package.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogMode {
    /// The catalog is consulted only for explicit `catalog:` specifiers;
    /// `add` / `update` never reconcile a direct version against it.
    #[default]
    Manual,

    /// A direct version that disagrees with the matching catalog entry is
    /// an error (`ERR_PNPM_CATALOG_VERSION_MISMATCH`).
    Strict,

    /// A direct version that disagrees with the matching catalog entry is
    /// kept, with a warning; a version that agrees is used from the
    /// catalog.
    Prefer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WarningPolicy {
    Warn,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogCleanup {
    Ignore,
    Prune,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSettings<Map> {
    pub rules: Map,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unused: Option<WarningPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSettings<Map> {
    pub entries: Map,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<CatalogMode>,
    #[serde(rename = "defaultSaveTarget", skip_serializing_if = "Option::is_none")]
    pub default_save_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unused: Option<CatalogCleanup>,
}

/// A package map in its shorthand or structured form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MapSetting<Section, Map> {
    Section(Section),
    Flat(Map),
}

pub type Rules<Map> = MapSetting<RuleSettings<Map>, Map>;
pub type DefaultCatalog<Map> = MapSetting<CatalogSettings<Map>, Map>;

impl<Section, Map> MapSetting<Section, Map> {
    pub fn section(&self) -> Option<&Section> {
        match self {
            Self::Section(section) => Some(section),
            Self::Flat(_) => None,
        }
    }
}

impl<Map> Rules<Map> {
    pub fn into_rules(self) -> Map {
        match self {
            Self::Section(section) => section.rules,
            Self::Flat(map) => map,
        }
    }

    pub fn rules_mut(&mut self) -> &mut Map {
        match self {
            Self::Section(section) => &mut section.rules,
            Self::Flat(map) => map,
        }
    }

    pub fn rules(&self) -> &Map {
        match self {
            Self::Section(section) => &section.rules,
            Self::Flat(map) => map,
        }
    }
}

impl<Map> DefaultCatalog<Map> {
    pub fn entries(&self) -> &Map {
        match self {
            Self::Section(section) => &section.entries,
            Self::Flat(map) => map,
        }
    }

    pub fn into_entries(self) -> Map {
        match self {
            Self::Section(section) => section.entries,
            Self::Flat(map) => map,
        }
    }
}

pub fn catalog_entries<'de, Deser: serde::Deserializer<'de>>(
    deserializer: Deser,
) -> Result<Option<pnpm_catalogs_types::Catalog>, Deser::Error> {
    Ok(specifiers::catalog(deserializer)?
        .map(|section| {
            section
                .into_entries()
                .into_iter()
                .collect()
        }))
}

/// The package map inside a serialized setting, in either supported form.
#[must_use]
pub fn package_map<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    package_map_value(value, field).as_object()
}

#[must_use]
pub fn package_map_value<'a>(value: &'a serde_json::Value, field: &str) -> &'a serde_json::Value {
    value
        .get(field)
        .filter(|entries| entries.is_object())
        .unwrap_or(value)
}
