//! Cache-aware metadata fetcher.
//!
//! When a cache directory is configured, the fetcher consults a
//! shared mirror under `<cache_dir>/v12/metadata-full/` (full) or
//! `<cache_dir>/v12/metadata/` (abbreviated), keyed by
//! `full_metadata`. It issues a conditional GET against the upstream
//! registry, and either reads the cached body (304) or writes the
//! new body back (2xx). Without a cache directory it falls through
//! to a plain GET.
//!
//! The directory layout matches pnpm's; the file format is pacquet's
//! own indexed shape (see [`crate::mirror`]) so warm loads hydrate
//! only the version fragments a pick consults.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use pnpm_network::{ThrottledClientGuard, redact_url_credentials, retry_async};
use pnpm_registry::Package;
use reqwest::{Response, StatusCode};

use crate::{
    FetchMetadataError,
    errors::legacy_mirror_hint,
    fetch_full_metadata::{
        MetadataRequestOptions, MetadataResponse, decode_metadata_response, send_metadata_request,
    },
    metadata_cache_policy::{DocumentKind, MirrorWrite},
    mirror::{
        ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, MetaHeaders, clear_meta,
        find_legacy_pkg_mirror, get_pkg_mirror_path, load_meta, load_meta_async,
        load_meta_headers_async, save_meta_indexed_with_headers, save_meta_ndjson_with_headers,
        scoped_meta_dir,
    },
    registry_url::to_registry_url,
};

/// Options bundle for [`fetch_full_metadata_cached`]. `cache_dir` is
/// the only addition over the no-cache
/// [`crate::FetchFullMetadataOptions`].
#[derive(Debug, Clone)]
pub struct FetchFullMetadataCachedOptions<'a> {
    pub registry: &'a str,
    /// When `Some`, the fetcher consults the on-disk mirror under
    /// the matching `<cache_dir>/v12/metadata...` subdirectory.
    /// When `None`, the fetcher short-circuits to an unconditional
    /// GET.
    pub cache_dir: Option<&'a Path>,
    /// `true` requests the full packument and caches it under
    /// [`FULL_META_DIR`]; `false` requests the abbreviated form
    /// and caches it under [`ABBREVIATED_META_DIR`].
    pub full_metadata: bool,
    /// When full metadata is requested, use pnpm's filtered metadata
    /// mirror and persist the filtered packument shape.
    pub filter_metadata: bool,
    /// When true, use only the on-disk metadata mirror and never reach
    /// the registry.
    pub offline: bool,
    /// Network-permit class the registry GET queues in:
    /// [`pnpm_network::UNPRIORITIZED`] for fetches that gate
    /// resolution progress, [`pnpm_network::BACKGROUND`] for the
    /// lockfile-verification fan-out.
    pub priority: u64,
    pub http: crate::MetadataHttpClient<'a>,
}

/// Fetch the full registry metadata document for `pkg_name`, reusing
/// the shared on-disk mirror when `cache_dir` is supplied.
///
/// Every version's mirror fragment is checked before the document is
/// returned, because its callers read across all versions and have no
/// fallback of their own. A damaged mirror fragment therefore reads as a
/// missing mirror:
/// offline it fails with `ERR_PNPM_NO_OFFLINE_META`, online the document
/// is refetched without the conditional cache, which rewrites the mirror.
pub async fn fetch_full_metadata_cached(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
) -> Result<Package, FetchMetadataError> {
    let meta = fetch_metadata_cached(pkg_name, opts, CachedFetch::MIRROR_DOCUMENT).await?;
    meta.versions.read_every_mirror_fragment();
    if !meta.versions.corrupt_mirror_fragment_found() {
        return Ok(meta);
    }
    refetch_damaged_mirror(pkg_name, opts).await
}

/// [`fetch_full_metadata_cached`] for a caller that keeps only `project`'s
/// result. `project` must read every version, through
/// [`pnpm_registry::PackageVersions::iter_policy_fields`], so the document is
/// never hydrated as a whole. A damaged mirror fragment found by the walk is
/// handled as [`fetch_full_metadata_cached`] handles it, and the refetched
/// document is projected instead.
pub(crate) async fn fetch_full_metadata_projected<Projection>(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
    project: impl Fn(&Package) -> Projection,
) -> Result<Projection, FetchMetadataError> {
    let meta = fetch_metadata_cached(pkg_name, opts, CachedFetch::MIRROR_DOCUMENT).await?;
    let projection = project(&meta);
    if !meta.versions.corrupt_mirror_fragment_found() {
        return Ok(projection);
    }
    drop(meta);
    refetch_damaged_mirror(pkg_name, opts).await.map(|meta| project(&meta))
}

async fn refetch_damaged_mirror(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
) -> Result<Package, FetchMetadataError> {
    if opts.offline {
        let url = to_registry_url(opts.registry, pkg_name);
        return Err(FetchMetadataError::NoOfflineMeta {
            pkg_name: pkg_name.to_string(),
            pkg_mirror: mirror_path_for(pkg_name, opts, &url).unwrap_or_default(),
            hint: None,
        });
    }
    fetch_metadata_cached(pkg_name, opts, CachedFetch::BYPASSING_MIRROR).await
}

/// [`fetch_full_metadata_cached`] without the up-front fragment check, for the
/// resolver, which checks the versions it reads itself.
pub(crate) async fn fetch_full_metadata_cached_lazily(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
) -> Result<FetchedDocument, FetchMetadataError> {
    fetch_document_cached(pkg_name, opts, CachedFetch::MIRROR_DOCUMENT).await
}

pub(crate) async fn fetch_full_metadata_bypassing_cache(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
) -> Result<FetchedDocument, FetchMetadataError> {
    fetch_document_cached(pkg_name, opts, CachedFetch::BYPASSING_MIRROR).await
}

/// A document the cached fetcher returned, with the kind it asked the
/// registry for. `requested` is `None` when it answered from the mirror
/// offline.
pub(crate) struct FetchedDocument {
    pub(crate) meta: Package,
    pub(crate) requested: Option<DocumentKind>,
}

/// Fetch the full document a `minimumReleaseAge` upgrade stores in the
/// abbreviated mirror `opts` names, as
/// [`DocumentKind::FullInAbbreviatedMirror`]. With `write_mirror` off (a
/// dry run) the mirror is read but never written or touched.
pub(crate) async fn fetch_release_age_upgrade(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
    write_mirror: bool,
) -> Result<Package, FetchMetadataError> {
    debug_assert!(!opts.full_metadata, "an upgrade fills the abbreviated mirror");
    let fetch = CachedFetch {
        document: CachedDocument::ReleaseAgeUpgrade,
        bypass_mirror: false,
        write_mirror,
    };
    fetch_metadata_cached(pkg_name, opts, fetch).await
}

/// What one [`fetch_metadata_cached`] call asks for and may do to the mirror.
#[derive(Debug, Clone, Copy)]
struct CachedFetch {
    document: CachedDocument,
    /// Ignore the mirror's validators, so only a body can answer.
    bypass_mirror: bool,
    write_mirror: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachedDocument {
    /// The document the mirror slot holds ([`DocumentKind::for_mirror`]).
    MirrorSlot,
    /// [`DocumentKind::FullInAbbreviatedMirror`].
    ReleaseAgeUpgrade,
}

impl CachedFetch {
    const MIRROR_DOCUMENT: Self = CachedFetch {
        document: CachedDocument::MirrorSlot,
        bypass_mirror: false,
        write_mirror: true,
    };
    const BYPASSING_MIRROR: Self = CachedFetch { bypass_mirror: true, ..Self::MIRROR_DOCUMENT };

    fn kind(self, full_metadata: bool, stored: Option<&MetaHeaders>) -> DocumentKind {
        match self.document {
            CachedDocument::MirrorSlot => DocumentKind::for_mirror(full_metadata, stored),
            CachedDocument::ReleaseAgeUpgrade => DocumentKind::FullInAbbreviatedMirror,
        }
    }
}

async fn fetch_metadata_cached(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
    fetch: CachedFetch,
) -> Result<Package, FetchMetadataError> {
    fetch_document_cached(pkg_name, opts, fetch).await.map(|document| document.meta)
}

async fn fetch_document_cached(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
    fetch: CachedFetch,
) -> Result<FetchedDocument, FetchMetadataError> {
    let url = to_registry_url(opts.registry, pkg_name);
    let mirror_path = mirror_path_for(pkg_name, opts, &url);

    if opts.offline {
        if let Some(meta) = load_meta_async(mirror_path.as_deref()).await {
            return Ok(FetchedDocument { meta, requested: None });
        }
        let hint = legacy_mirror_for(pkg_name, opts).await.map(|path| legacy_mirror_hint(&path));
        return Err(FetchMetadataError::NoOfflineMeta {
            pkg_name: pkg_name.to_string(),
            pkg_mirror: mirror_path.unwrap_or_default(),
            hint,
        });
    }

    let cache_headers = if fetch.bypass_mirror {
        None
    } else {
        load_meta_headers_async(mirror_path.as_deref()).await
    };
    let kind = fetch.kind(opts.full_metadata, cache_headers.as_ref());
    let attempt = FetchAttempt {
        pkg_name,
        url: &url,
        opts,
        mirror_path: mirror_path.as_deref(),
        kind,
        write_mirror: fetch.write_mirror,
        cache_headers,
        cache_bypass: AtomicBool::new(fetch.bypass_mirror),
    };
    let meta =
        retry_async(&url, opts.http.retry_opts, FetchMetadataError::is_transient, || attempt.run())
            .await?;
    Ok(FetchedDocument { meta, requested: Some(kind) })
}

/// One conditional metadata fetch, re-entered from the top by each body
/// retry.
struct FetchAttempt<'a> {
    pkg_name: &'a str,
    url: &'a str,
    opts: &'a FetchFullMetadataCachedOptions<'a>,
    mirror_path: Option<&'a Path>,
    kind: DocumentKind,
    write_mirror: bool,
    cache_headers: Option<MetaHeaders>,
    cache_bypass: AtomicBool,
}

impl FetchAttempt<'_> {
    async fn run(&self) -> Result<Package, FetchMetadataError> {
        let opts = self.opts;
        let request = self.metadata_request();
        let (client, response) = send_metadata_request(&request).await?;

        let (client, response) = if response.status() == StatusCode::NOT_MODIFIED {
            let recovery = NotModifiedContext {
                mirror_path: self.mirror_path,
                cache_bypass: &self.cache_bypass,
                renew_mirror: self.write_mirror,
            };
            match recover_from_not_modified(client, &request, recovery).await? {
                NotModifiedRecovery::Serve(meta) => return Ok(meta),
                NotModifiedRecovery::Refetched(client, response) => (client, response),
            }
        } else {
            (client, response)
        };

        let store = self.mirror_store(&response);
        decode_metadata_response(
            MetadataResponse {
                client,
                response,
                url: self.url,
                http_client: opts.http.http_client,
            },
            store.mirror_write.normalizes_to_abbreviated(),
            move |meta| store.store(meta),
        )
        .await
    }

    fn mirror_store(&self, response: &Response) -> MirrorStore {
        MirrorStore {
            url: self.url.to_string(),
            mirror_path: self.mirror_path.filter(|_| self.write_mirror).map(Path::to_path_buf),
            mirror_write: MirrorWrite::on_response(self.kind, response.headers()),
            should_filter_metadata: self.opts.full_metadata && self.opts.filter_metadata,
        }
    }

    fn metadata_request(&self) -> MetadataRequestOptions<'_> {
        let opts = self.opts;
        let revalidation = self.kind.revalidation(
            self.cache_headers.as_ref(),
            self.cache_bypass.load(Ordering::Relaxed),
        );
        MetadataRequestOptions {
            pkg_name: self.pkg_name,
            url: self.url,
            accept: self.kind.accept(),
            priority: opts.priority,
            etag: revalidation.etag,
            modified: revalidation.modified,
            bypass_cache: revalidation.bypass_cache,
            http: opts.http.one_attempt(),
        }
    }
}

/// Where this route's mirror lives, or `None` when there is no cache
/// directory or the path could not be encoded.
///
/// The route is classified once so the mirror lands in the namespace the
/// route policy permits: the global mirror for a public route and a
/// descriptor-scoped private mirror for a proxied/hosted route.
///
/// Encoding the mirror path can fail only on a malformed registry URL (no
/// host, unparsable). Either case is a config bug; it is logged and the
/// fetch proceeds without a cache so the user still gets metadata on this
/// install instead of a hard error.
fn mirror_path_for(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
    url: &str,
) -> Option<PathBuf> {
    let base_meta_dir = if opts.full_metadata {
        if opts.filter_metadata { FULL_FILTERED_META_DIR } else { FULL_META_DIR }
    } else {
        ABBREVIATED_META_DIR
    };
    let scope = opts.http.auth_headers.metadata_scope(url, Some(pkg_name));
    let meta_dir = scoped_meta_dir(&scope, base_meta_dir);
    match get_pkg_mirror_path(opts.cache_dir?, &meta_dir, opts.registry, pkg_name) {
        Ok(path) => Some(path),
        Err(error) => {
            tracing::debug!(
                target: "pnpm_resolving_npm_resolver::cache",
                ?error,
                registry = opts.registry,
                pkg_name,
                full_metadata = opts.full_metadata,
                "could not encode mirror path; bypassing cache for this call",
            );
            None
        }
    }
}

/// Locate a legacy mirror of `pkg_name` in `opts.cache_dir`.
/// Unlike [`mirror_path_for`], this checks only the unscoped directory.
async fn legacy_mirror_for(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
) -> Option<PathBuf> {
    let base_meta_dir = if opts.full_metadata {
        if opts.filter_metadata { FULL_FILTERED_META_DIR } else { FULL_META_DIR }
    } else {
        ABBREVIATED_META_DIR
    };
    find_legacy_pkg_mirror(opts.cache_dir?, base_meta_dir, opts.registry, pkg_name).await
}

/// The off-reactor end of one fetch: filter the parsed document and persist
/// the mirror.
struct MirrorStore {
    url: String,
    mirror_path: Option<PathBuf>,
    mirror_write: MirrorWrite,
    should_filter_metadata: bool,
}

impl MirrorStore {
    fn store(self, mut meta: Package) -> Result<Package, FetchMetadataError> {
        if self.should_filter_metadata {
            meta = clear_meta(&meta)
                .map_err(|error| FetchMetadataError::FilterMetadata {
                    url: redact_url_credentials(&self.url),
                    error: error.into_inner(),
                })?;
        }
        match self.persist(&meta) {
            // Serve the just-persisted mirror instead of the response body:
            // its version fragments read from the file on demand, so the
            // multi-megabyte body drops here instead of living in the
            // packument cache for the rest of the install.
            Some(saved) => Ok(saved),
            None => Ok(meta),
        }
    }

    /// Write the mirror, returning the packument re-read from it when the
    /// indexed layout could serve later reads lazily.
    ///
    /// A filtered full response is written in pnpm's NDJSON shape. Other
    /// responses keep pacquet's indexed mirror layout for lazy version
    /// hydration.
    fn persist(&self, meta: &Package) -> Option<Package> {
        let path = self.mirror_path.as_deref()?;
        let headers = self.mirror_write.headers(meta);
        if self.should_filter_metadata {
            if let Err(error) = save_meta_ndjson_with_headers(path, meta, &headers) {
                warn_mirror_write_failed(&error, path);
                self.mirror_write.discard_after_failed_save(path);
            }
            return None;
        }
        if let Err(error) = save_meta_indexed_with_headers(path, meta, &headers) {
            warn_mirror_write_failed(&error, path);
            self.mirror_write.discard_after_failed_save(path);
            return None;
        }
        load_meta(path)
    }
}

fn warn_mirror_write_failed(error: &impl std::fmt::Debug, path: &Path) {
    tracing::debug!(
        target: "pnpm_resolving_npm_resolver::cache",
        ?error,
        path = %path.display(),
        "could not persist mirror; bypassing cache write",
    );
}

/// The mirror a `304 Not Modified` is honoured against.
#[derive(Clone, Copy)]
struct NotModifiedContext<'a> {
    mirror_path: Option<&'a Path>,
    cache_bypass: &'a AtomicBool,
    /// Whether the fetch may touch the mirror ([`renew_mirror_freshness`]).
    renew_mirror: bool,
}

/// Outcome of a `304 Not Modified` once it is honoured against the mirror.
enum NotModifiedRecovery<'a> {
    /// The mirror body was still on disk; serve it.
    Serve(Package),
    /// The mirror had vanished, so a cache-bypassing refetch returned a body.
    Refetched(ThrottledClientGuard<'a>, Response),
}

/// Honour a `304 Not Modified` against the on-disk mirror.
///
/// Serves the mirror body when it is still present. When the mirror vanished
/// after the conditional request (concurrent store cleanup, antivirus, ...)
/// the 304 validates nothing, so re-request once as a cold cache would —
/// flipping `cache_bypass` so the retrying [`retry_async`] loop stays bypassed
/// too — which the registry can only answer with a body or an error, never
/// another 304. A 304 with no mirror at all is `META_NOT_MODIFIED_WITHOUT_CACHE`.
async fn recover_from_not_modified<'a>(
    client: ThrottledClientGuard<'a>,
    request: &MetadataRequestOptions<'a>,
    recovery: NotModifiedContext<'_>,
) -> Result<NotModifiedRecovery<'a>, FetchMetadataError> {
    let NotModifiedContext {
        mirror_path,
        cache_bypass,
        renew_mirror,
    } = recovery;
    // No body to stream — release the connection and its network-concurrency
    // permit before the mirror disk read.
    drop(client);
    let Some(path) = mirror_path else {
        return Err(FetchMetadataError::NotModifiedWithoutCache {
            pkg_name: request.pkg_name.to_string(),
        });
    };
    if let Some(meta) = load_meta_async(Some(path)).await {
        if renew_mirror {
            renew_mirror_freshness(path);
        }
        return Ok(NotModifiedRecovery::Serve(meta));
    }
    cache_bypass.store(true, Ordering::Relaxed);
    let (client, response) = send_metadata_request(&MetadataRequestOptions {
        pkg_name: request.pkg_name,
        url: request.url,
        accept: request.accept,
        priority: request.priority,
        etag: None,
        modified: None,
        bypass_cache: true,
        http: request.http,
    })
    .await?;
    Ok(NotModifiedRecovery::Refetched(client, response))
}

/// Bump the mirror file's mtime to "now" after a `304 Not Modified`.
///
/// The publishedBy mtime shortcut in [`crate::pick_package()`] treats
/// a mirror younger than the maturity cutoff as authoritative. A 304
/// proves the cached packument equals the registry's current one, so
/// the validation clock legitimately restarts here — without the
/// touch, a mirror older than `minimumReleaseAge` re-validates every
/// package on every subsequent install, because a 304 never rewrites
/// the file.
///
/// The append-mode open carries the write-attributes access Windows'
/// `set_modified` needs (a read-only handle cannot set file times
/// there). The read-only fallback covers Unix mirrors whose mode
/// dropped write permission: `futimens`-style timestamp syscalls
/// require ownership, not write access. Best-effort: a failure only
/// costs the next install another conditional request.
fn renew_mirror_freshness(path: &Path) {
    let touched = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .or_else(|_| std::fs::File::open(path))
        .and_then(|file| file.set_modified(std::time::SystemTime::now()));
    if let Err(error) = touched {
        tracing::debug!(
            target: "pnpm_resolving_npm_resolver::cache",
            ?error,
            path = %path.display(),
            "could not renew mirror freshness after 304",
        );
    }
}

#[cfg(test)]
mod tests;
