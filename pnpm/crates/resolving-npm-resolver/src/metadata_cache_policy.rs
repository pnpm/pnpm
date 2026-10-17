//! The registry metadata cache's decisions, kept free of I/O:
//!
//! - [`DocumentKind`]: which representation a request asks for.
//! - [`DocumentKind::revalidation`]: which validators and cache directives it
//!   sends.
//! - [`MirrorWrite`]: how a `2xx` response is recorded in the mirror. It is
//!   the only writer of a fetched document's [`MetaHeaders`].
//! - [`mirror_may_answer`]: whether a pick may be answered from the mirror
//!   without the registry.

use std::path::Path;

use pnpm_config::NetworkPolicy;
use pnpm_registry::Package;
use reqwest::header::{self, HeaderMap};

use crate::{
    fetch_full_metadata::{
        ACCEPT_ABBREVIATED_DOC, ACCEPT_FULL_DOC, is_abbreviated_content_type,
        metadata_response_is_uncacheable,
    },
    mirror::MetaHeaders,
};

/// The representation a metadata request asks for and a mirror records.
///
/// An entity tag and a `Last-Modified` date describe one representation, so
/// a mirror's validators are sent only for the kind that recorded them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    /// The abbreviated `install-v1` document, in the abbreviated mirror.
    Abbreviated,
    /// The full document, in a full mirror.
    Full,
    /// The full document in the abbreviated mirror, which a
    /// `minimumReleaseAge` upgrade stores there. Its entity tag is
    /// [`MetaHeaders::full_etag`].
    FullInAbbreviatedMirror,
}

impl DocumentKind {
    /// The document a request outside any mirror asks for.
    pub(crate) fn requested(full_metadata: bool) -> Self {
        if full_metadata { DocumentKind::Full } else { DocumentKind::Abbreviated }
    }

    /// The document a fetch for a mirror asks for. An abbreviated mirror that
    /// holds an upgraded full document keeps asking for the full document, so
    /// a registry with per-representation tags can answer `304`.
    pub(crate) fn for_mirror(full_metadata: bool, stored: Option<&MetaHeaders>) -> Self {
        if full_metadata {
            return DocumentKind::Full;
        }
        let holds_full_document = stored
            .and_then(|headers| headers.full_etag.as_deref())
            .is_some_and(|etag| !etag.is_empty());
        if holds_full_document {
            DocumentKind::FullInAbbreviatedMirror
        } else {
            DocumentKind::Abbreviated
        }
    }

    pub(crate) fn accept(self) -> &'static str {
        match self {
            DocumentKind::Abbreviated => ACCEPT_ABBREVIATED_DOC,
            DocumentKind::Full | DocumentKind::FullInAbbreviatedMirror => ACCEPT_FULL_DOC,
        }
    }

    /// The request for this kind of document over `stored`, the header line
    /// of the mirror slot it fills.
    ///
    /// - `bypass` drops the mirror: no validators, and `Cache-Control:
    ///   no-cache`, so only a body can answer.
    /// - A mirror that recorded another kind sends no validators: they
    ///   describe the other representation.
    /// - A mirror the registry marked uncacheable keeps its validators and
    ///   adds `Cache-Control: no-cache`, so an intermediary must ask the
    ///   origin, which can still answer `304`.
    pub(crate) fn revalidation(
        self,
        stored: Option<&MetaHeaders>,
        bypass: bool,
    ) -> Revalidation<'_> {
        let recorded_here = |stored: &&MetaHeaders| {
            DocumentKind::for_mirror(self == DocumentKind::Full, Some(stored)) == self
        };
        let Some(stored) = stored.filter(|_| !bypass).filter(recorded_here) else {
            return Revalidation { etag: None, modified: None, bypass_cache: bypass };
        };
        let etag = match self {
            DocumentKind::FullInAbbreviatedMirror => stored.full_etag.as_deref(),
            DocumentKind::Abbreviated | DocumentKind::Full => stored.etag.as_deref(),
        };
        Revalidation {
            etag,
            modified: stored.modified.as_deref(),
            bypass_cache: stored.uncacheable,
        }
    }
}

/// The validators and the `Cache-Control: no-cache` choice of one metadata
/// request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Revalidation<'a> {
    pub etag: Option<&'a str>,
    pub modified: Option<&'a str>,
    pub bypass_cache: bool,
}

/// How a `2xx` metadata response is recorded in the mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MirrorWrite {
    kind: DocumentKind,
    etag: Option<String>,
    /// The response `Cache-Control` forbade reusing this document (see
    /// [`metadata_response_is_uncacheable`]).
    uncacheable: bool,
    normalize_to_abbreviated: bool,
}

impl MirrorWrite {
    /// Reads the response headers of a `kind` request.
    pub(crate) fn on_response(kind: DocumentKind, headers: &HeaderMap) -> Self {
        MirrorWrite {
            kind,
            etag: headers
                .get(header::ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            uncacheable: metadata_response_is_uncacheable(headers),
            normalize_to_abbreviated: normalizes_to_abbreviated(kind, headers),
        }
    }

    /// [`normalizes_to_abbreviated`] for this response.
    pub(crate) fn normalizes_to_abbreviated(&self) -> bool {
        self.normalize_to_abbreviated
    }

    /// The header line of a mirror that stores `meta` from this response.
    pub(crate) fn headers(&self, meta: &Package) -> MetaHeaders {
        let headers = MetaHeaders::new(meta, None, self.uncacheable);
        match self.kind {
            DocumentKind::FullInAbbreviatedMirror => {
                MetaHeaders { full_etag: self.etag.clone(), ..headers }
            }
            DocumentKind::Abbreviated | DocumentKind::Full => {
                MetaHeaders { etag: self.etag.clone(), ..headers }
            }
        }
    }

    /// Clean up after the mirror at `path` could not be written. The previous
    /// header line is left, and the next fetch would send its validators. A
    /// response that forbade caching must not leave them, so that mirror is
    /// removed.
    pub(crate) fn discard_after_failed_save(&self, path: &Path) {
        if self.uncacheable {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Whether a `2xx` answer to a `kind` request is the full document a
/// registry served while ignoring the abbreviated `Accept` header, which
/// [`crate::fetch_full_metadata::normalize_abbreviated_meta`] strips.
pub(crate) fn normalizes_to_abbreviated(kind: DocumentKind, headers: &HeaderMap) -> bool {
    kind == DocumentKind::Abbreviated && !is_abbreviated_content_type(headers)
}

/// Whether a pick may be answered from the mirror without asking the
/// registry. An online pick asks the registry when it forbade caching the
/// mirror's document. Offline and prefer-offline still read the mirror.
pub(crate) fn mirror_may_answer(network: NetworkPolicy, stored: Option<&MetaHeaders>) -> bool {
    network.reads_cache_first() || !stored.is_some_and(|headers| headers.uncacheable)
}

#[cfg(test)]
mod tests;
