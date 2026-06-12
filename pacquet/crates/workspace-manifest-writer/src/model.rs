//! Parsed view of a `pnpm-workspace.yaml` used to decide catalog edits.
//!
//! Holds the original text verbatim (so untouched bytes survive) alongside
//! the decoded top-level key order and catalog data the edit pass consults.

use indexmap::IndexMap;
use serde::Deserialize;

/// The `catalog:` / `catalogs:` slice of a `pnpm-workspace.yaml`, decoded
/// twice over the same source: once for the ordered top-level key list, once
/// for the catalog values.
pub(crate) struct Manifest {
    text: String,
    pub(crate) top_level_keys: Vec<String>,
    /// `catalog:` shorthand for the default catalog.
    pub(crate) catalog: Option<IndexMap<String, String>>,
    /// `catalogs:` map of named catalogs (may include `default`).
    pub(crate) catalogs: Option<IndexMap<String, IndexMap<String, String>>>,
    /// `configDependencies:` clean-specifier entries. Object-form
    /// entries (the legacy `{ tarball?, integrity }` shape) are dropped
    /// here — they're only consulted to detect a no-op write of an
    /// already-present clean specifier.
    pub(crate) config_dependencies: Option<IndexMap<String, String>>,
}

#[derive(Default, Deserialize)]
struct CatalogData {
    #[serde(default)]
    catalog: Option<IndexMap<String, String>>,
    #[serde(default)]
    catalogs: Option<IndexMap<String, IndexMap<String, String>>>,
    #[serde(default, rename = "configDependencies")]
    config_dependencies: Option<IndexMap<String, ConfigDepValue>>,
}

/// A `configDependencies` value, tolerant of the legacy object form so
/// decoding a manifest that uses it doesn't fail. Only the clean-string
/// shape is retained.
#[derive(Deserialize)]
#[serde(untagged)]
enum ConfigDepValue {
    Clean(String),
    Other(serde::de::IgnoredAny),
}

impl Manifest {
    /// Parse `original` (the file's contents, or `None` when the file is
    /// absent). An empty or whitespace/comment-only document decodes to an
    /// empty manifest, matching pnpm treating a nil parse as `{}`.
    pub(crate) fn parse(original: Option<&str>) -> Result<Self, Box<serde_saphyr::Error>> {
        let text = original.unwrap_or_default().to_string();

        if text.trim().is_empty() {
            return Ok(Manifest {
                text,
                top_level_keys: Vec::new(),
                catalog: None,
                catalogs: None,
                config_dependencies: None,
            });
        }

        let top: Option<IndexMap<String, serde::de::IgnoredAny>> =
            serde_saphyr::from_str(&text).map_err(Box::new)?;
        let top_level_keys = top.map(|map| map.into_keys().collect()).unwrap_or_default();

        let data: CatalogData = serde_saphyr::from_str(&text).map_err(Box::new)?;
        let config_dependencies = data.config_dependencies.map(|entries| {
            entries
                .into_iter()
                .filter_map(|(name, value)| match value {
                    ConfigDepValue::Clean(specifier) => Some((name, specifier)),
                    ConfigDepValue::Other(_) => None,
                })
                .collect()
        });

        Ok(Manifest {
            text,
            top_level_keys,
            catalog: data.catalog,
            catalogs: data.catalogs,
            config_dependencies,
        })
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub(crate) fn into_text(self) -> String {
        self.text
    }
}
