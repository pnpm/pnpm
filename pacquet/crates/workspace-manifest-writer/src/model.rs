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
}

#[derive(Default, Deserialize)]
struct CatalogData {
    #[serde(default)]
    catalog: Option<IndexMap<String, String>>,
    #[serde(default)]
    catalogs: Option<IndexMap<String, IndexMap<String, String>>>,
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
            });
        }

        let top: Option<IndexMap<String, serde::de::IgnoredAny>> =
            serde_saphyr::from_str(&text).map_err(Box::new)?;
        let top_level_keys = top.map(|map| map.into_keys().collect()).unwrap_or_default();

        let data: CatalogData = serde_saphyr::from_str(&text).map_err(Box::new)?;

        Ok(Manifest { text, top_level_keys, catalog: data.catalog, catalogs: data.catalogs })
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
