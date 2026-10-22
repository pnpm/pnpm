//! Encoding a tar registry's tarball response for the request.
//!
//! - `gzip`: the `.tgz` itself, which is the gzip encoding of its tar.
//! - `zstd`: the stored level-19 variant (see [`super::tar_variants`]), or
//!   until one exists the decoded tar, which the response compression layer
//!   encodes at its default level.
//! - neither: the decoded tar.
//!
//! A cached upstream `.tgz` is served without a re-check against the current
//! packument, and a client of a tar registry may have no tar hash to check what
//! it receives. Its bytes are checked against the version's `.tgz` integrity
//! as they stream, and a mismatch fails the response. Hosted and freshly
//! fetched `.tgz` files are already verified, so they are not hashed again.

pub(super) use decode::{
    MAX_CONCURRENT_TAR_DECODES, tar_decode_limit, tar_integrity_of, tar_integrity_within,
    tar_resource_integrity,
};

use super::{AppState, CanonicalPackageName, HeaderMap, Response, TarCopy, header, open_variant};
use axum::body::Body;
use decode::{DerivedFrom, gunzip_body};
use negotiation::{TarCoding, preferred_coding};
use pnpr_storage::{Storage, streaming};
use ssri::Integrity;
use verify::{CheckedReader, StaleTgz, verified_body};

mod decode;
mod negotiation;
mod verify;

/// The version a tar response serves.
pub(super) struct TarSource {
    /// The package, which keys the level-19 variant and the tar-integrity
    /// table. `None` for a revision fetched from an upstream by its digest.
    pub(super) name: Option<CanonicalPackageName>,
    /// The version's `.tgz` integrity, which keys what is derived from it:
    /// the tar's integrity and the level-19 variant.
    pub(super) tgz_integrity: Option<String>,
    /// Whether the source may persist package content. A `cache: false`
    /// upstream may not, so it gets no variants.
    pub(super) keeps_content: bool,
    pub(super) trust: TgzTrust,
    /// Whether the tar-integrity table already records this `.tgz`'s tar.
    pub(super) tar_integrity_known: bool,
    /// See [`decode::tar_decode_limit`].
    pub(super) decode_limit: u64,
}

/// What is known about the `.tgz` bytes a tar response serves.
pub(super) enum TgzTrust {
    /// Hosted, or fetched from the upstream and verified as it streamed.
    Verified,
    /// A cached upstream copy, which may no longer match the packument. A
    /// mismatch evicts it so the next request fetches the current bytes.
    Cached(CachedTgz),
}

pub(super) struct CachedTgz {
    pub(super) namespace: String,
    pub(super) filename: String,
}

/// Serve the tar of the `.tgz` response `tgz`, encoded as the request
/// accepts.
pub(super) async fn tar_response(
    state: &AppState,
    headers: &HeaderMap,
    tgz: Response,
    source: TarSource,
) -> Response {
    let storage = &state.inner.storage;
    match preferred_coding(headers) {
        TarCoding::Zstd => {
            let mut copy = None;
            if let (Some(name), Some(key)) = (&source.name, source.tgz_integrity.as_deref())
                && source.keeps_content
            {
                if let Some((file, len)) = open_variant(storage, name, key).await {
                    return encoded_response(tgz, streaming::stream_file(file), Some(len), "zstd");
                }
                copy = TarCopy::start(&state.inner.proxy.tar_variants, storage, name, key).await;
            }
            decoded_response(state, tgz, source, copy)
        }
        TarCoding::Gzip => gzip_response(storage.clone(), tgz, source),
        TarCoding::Identity => decoded_response(state, tgz, source, None),
    }
}

impl TarSource {
    /// The integrity the served bytes still have to be checked against.
    fn check(&self) -> Option<Integrity> {
        match self.trust {
            TgzTrust::Cached(_) => self.tgz_integrity
                .as_deref()?
                .parse()
                .ok(),
            TgzTrust::Verified => None,
        }
    }

    fn stale(self, storage: Storage) -> StaleTgz {
        StaleTgz {
            storage,
            name: self.name,
            cached: match self.trust {
                TgzTrust::Cached(cached) => Some(cached),
                TgzTrust::Verified => None,
            },
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

/// The `.tgz` itself as a gzip-encoded tar.
fn gzip_response(storage: Storage, tgz: Response, source: TarSource) -> Response {
    let (parts, body) = tgz.into_parts();
    let check = source.check();
    // A client stops reading at a declared length, so a mismatch reported
    // after the last byte would reach it only as a dropped connection it never
    // sees. Without the length, the missing final chunk fails it.
    let len = parts.headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .filter(|_| check.is_none());
    let body = verified_body(body, check, source.stale(storage));
    encoded_response(Response::from_parts(parts, Body::empty()), body, len, "gzip")
}

/// `tgz`'s status and caller-scoping headers with an already encoded tar body.
fn encoded_response(tgz: Response, body: Body, len: Option<u64>, coding: &'static str) -> Response {
    let (mut parts, _) = tgz.into_parts();
    describe_tar_headers(&mut parts.headers);
    parts.headers.insert(header::CONTENT_ENCODING, header::HeaderValue::from_static(coding));
    if let Some(len) = len {
        parts.headers.insert(header::CONTENT_LENGTH, len.into());
    }
    Response::from_parts(parts, body)
}

/// Serve the tar `tgz` decodes to. Once the whole of it decoded, and a cached
/// `.tgz` matched the version's integrity, record the tar's integrity and hand
/// `copy` to the variant worker.
fn decoded_response(
    state: &AppState,
    tgz: Response,
    source: TarSource,
    copy: Option<TarCopy>,
) -> Response {
    let storage = state.inner.storage.clone();
    let (mut parts, body) = tgz.into_parts();
    describe_tar_headers(&mut parts.headers);
    let tgz_integrity = source.tgz_integrity.clone();
    let name = source.name.clone();
    let derived = DerivedFrom {
        check: source.check(),
        hash: tgz_integrity.is_some() && !source.tar_integrity_known,
        copy,
        limit: source.decode_limit,
        stale: source.stale(storage.clone()),
    };
    let runtime = tokio::runtime::Handle::current();
    let decodes = std::sync::Arc::clone(&state.inner.proxy.tar_decodes);
    let body = gunzip_body(body, derived, decodes, move |tar_integrity, inspection| {
        let (Some(key), Some(name)) = (tgz_integrity, name) else { return };
        runtime.spawn(async move {
            let derived = pnpr_storage::DerivedTar::new(
                tar_integrity.to_string(),
                inspection.requires_build,
                inspection.file_count,
                inspection.unpacked_size,
            )
            .with_bins(inspection.bins);
            if let Err(err) = storage.record_tar_integrity(&name, &key, derived).await {
                tracing::warn!(?err, package = %name.as_str(), "recording a tar integrity failed");
            }
        });
    });
    Response::from_parts(parts, body)
}

/// Replace the `.tgz` response's representation headers with a tar's.
fn describe_tar_headers(headers: &mut HeaderMap) {
    for stale in [header::CONTENT_LENGTH, header::ETAG, header::ACCEPT_RANGES] {
        headers.remove(stale);
    }
    headers.remove("content-digest");
    headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/x-tar"));
    headers.append(header::VARY, header::HeaderValue::from_static("accept-encoding"));
}

#[cfg(test)]
mod tests;
