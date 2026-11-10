//! The `catalogs` of the next format's `settings`. An entry is its specifier
//! alone when the version follows from it, as an importer's dependency is,
//! and `{specifier, version}` on one line otherwise.
//!
//! A catalog entry records the bare version of an `npm:` alias, so
//! `npm:name@1.2.3` implies `1.2.3` here, where an importer's dependency
//! implies `name@1.2.3`.

use super::NextFormatError;
use crate::{CatalogSnapshots, ResolvedCatalogEntry, implied_version::catalog_implied_version};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::collections::BTreeMap;

/// One entry of a catalog.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NextCatalogEntry {
    Specifier(String),
    Resolved(ResolvedCatalogEntry),
}

pub type NextCatalogs = BTreeMap<String, BTreeMap<String, NextCatalogEntry>>;

/// A string is the specifier and a map the resolved form, read directly
/// rather than through an untagged enum's buffering.
impl<'de> Deserialize<'de> for NextCatalogEntry {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        struct EntryVisitor;

        impl<'de> Visitor<'de> for EntryVisitor {
            type Value = NextCatalogEntry;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a specifier, or a map of specifier and version")
            }

            fn visit_str<Error: de::Error>(self, text: &str) -> Result<NextCatalogEntry, Error> {
                Ok(NextCatalogEntry::Specifier(text.to_owned()))
            }

            fn visit_string<Error: de::Error>(
                self,
                text: String,
            ) -> Result<NextCatalogEntry, Error> {
                Ok(NextCatalogEntry::Specifier(text))
            }

            fn visit_map<Map: MapAccess<'de>>(
                self,
                map: Map,
            ) -> Result<NextCatalogEntry, Map::Error> {
                ResolvedCatalogEntry::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(NextCatalogEntry::Resolved)
            }
        }

        deserializer.deserialize_any(EntryVisitor)
    }
}

/// `catalogs` with every entry whose version follows from its specifier
/// written as the specifier.
pub(super) fn collapsed(catalogs: &CatalogSnapshots) -> NextCatalogs {
    catalogs
        .iter()
        .map(|(catalog, entries)| {
            let entries = entries
                .iter()
                .map(|(name, entry)| (name.clone(), collapse(entry)))
                .collect();
            (catalog.clone(), entries)
        })
        .collect()
}

fn collapse(entry: &ResolvedCatalogEntry) -> NextCatalogEntry {
    if catalog_implied_version(&entry.specifier) == Some(entry.version.as_str()) {
        NextCatalogEntry::Specifier(entry.specifier.clone())
    } else {
        NextCatalogEntry::Resolved(entry.clone())
    }
}

/// `catalogs` with every version written out. Fails on an entry whose
/// version does not follow from its specifier.
pub(super) fn resolved(catalogs: NextCatalogs) -> Result<CatalogSnapshots, NextFormatError> {
    catalogs
        .into_iter()
        .map(|(catalog, entries)| {
            let entries = entries
                .into_iter()
                .map(|(name, entry)| Ok((resolve(&catalog, &name, entry)?, name)))
                .map(|entry| entry.map(|(entry, name)| (name, entry)))
                .collect::<Result<_, NextFormatError>>()?;
            Ok((catalog, entries))
        })
        .collect()
}

fn resolve(
    catalog: &str,
    name: &str,
    entry: NextCatalogEntry,
) -> Result<ResolvedCatalogEntry, NextFormatError> {
    let specifier = match entry {
        NextCatalogEntry::Resolved(entry) => return Ok(entry),
        NextCatalogEntry::Specifier(specifier) => specifier,
    };
    let version = catalog_implied_version(&specifier)
        .ok_or_else(|| NextFormatError::UnderivableCatalogVersion {
            catalog: catalog.to_string(),
            name: name.to_string(),
            specifier: specifier.clone(),
        })?
        .to_string();
    Ok(ResolvedCatalogEntry { specifier, version })
}
