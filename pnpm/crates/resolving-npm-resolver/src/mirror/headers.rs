use pnpm_registry::Package;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Cached headers persisted as the mirror's first line. The cached
/// metadata fetcher feeds these into `If-None-Match` /
/// `If-Modified-Since` on the next request. Both fields are
/// optional because some registries omit one or the other; the
/// fetcher tolerates a partial header set and only sends the headers
/// it has.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaHeaders {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// The entity tag of a full document that a `minimumReleaseAge` upgrade
    /// stored in the abbreviated mirror. It validates only the full
    /// representation, so the next request for this mirror asks for the full
    /// document and sends this tag instead of [`Self::etag`].
    #[serde(default, rename = "fullEtag", skip_serializing_if = "Option::is_none")]
    pub full_etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// The registry's `Cache-Control` said this document is already stale
    /// (`max-age=0`, `no-cache`, or `no-store`). The next online fetch must
    /// not revalidate it with `If-None-Match` or `If-Modified-Since`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub uncacheable: bool,
}

impl MetaHeaders {
    /// The headers a mirror of `meta` records when `etag` tags the
    /// representation the mirror holds.
    #[must_use]
    pub fn new(meta: &Package, etag: Option<&str>, uncacheable: bool) -> Self {
        MetaHeaders {
            etag: etag.map(str::to_string),
            full_etag: None,
            modified: meta_modified(meta),
            uncacheable,
        }
    }

    /// The headers an abbreviated mirror records for the full document
    /// `meta`, tagged `full_etag`.
    #[must_use]
    pub fn for_full_meta_in_abbreviated_mirror(
        meta: &Package,
        full_etag: Option<&str>,
        uncacheable: bool,
    ) -> Self {
        MetaHeaders {
            full_etag: full_etag.map(str::to_string),
            ..Self::new(meta, None, uncacheable)
        }
    }
}

fn meta_modified(meta: &Package) -> Option<String> {
    meta.modified
        .clone()
        .or_else(|| {
            meta.time
                .as_ref()
                .and_then(|time| time.get("modified"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}
