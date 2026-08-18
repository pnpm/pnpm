//! Error types for tarball download, verification, and extraction.

use derive_more::{Display, Error, From};
use miette::Diagnostic;
use pnpm_store_dir::{StoreIndexError, WriteCasFileError};
use std::path::PathBuf;
use zune_inflate::errors::InflateDecodeErrors;

/// Reqwest's own [`std::fmt::Display`] for a request-stage failure renders as
/// `error sending request for url (URL): <inner>` only if it can find
/// an inner source, and on some failure modes (e.g. the request was
/// dropped before a connect was attempted) `inner` is `None` —
/// leaving the user with the truly opaque `error sending request for
/// url (URL)` and no clue about what actually failed.
///
/// [`walk_reqwest_chain`] walks `error.source()` itself and joins every
/// stage's `Display` with `: ` so the rendered [`NetworkError`] always
/// carries the leaf reason (e.g. `Connection refused (os error 61)`,
/// `tls handshake eof`, `dns error: failed to lookup address`),
/// regardless of which intermediate `reqwest` / `hyper` / `io::Error`
/// happens to elide it.
fn walk_reqwest_chain(error: &reqwest::Error) -> String {
    let mut out = error.to_string();
    let mut error: &dyn std::error::Error = error;
    while let Some(src) = error.source() {
        let frame = src.to_string();
        // Skip empty or duplicate frames — hyper occasionally repeats
        // the same message across two layers, and reqwest sometimes
        // already includes the inner string in its top-level Display.
        if !frame.is_empty() && !out.ends_with(&frame) {
            out.push_str(": ");
            out.push_str(&frame);
        }
        error = src;
    }
    out
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to fetch {url}: {}", walk_reqwest_chain(error))]
pub struct NetworkError {
    pub url: String,
    /// Marked `#[error(source)]` so miette can also walk the chain on
    /// its own (some renderers prefer the structured form). The
    /// flattened string in `Display` is for the default miette report
    /// where the user just sees one line per wrapper.
    #[error(source)]
    pub error: reqwest::Error,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Tarball server returned HTTP {status} for {url}")]
pub struct HttpStatusError {
    pub url: String,
    pub status: u16,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to verify the integrity of {url}: {error}")]
pub struct VerifyChecksumError {
    pub url: String,
    #[error(source)]
    pub error: ssri::Error,
}

#[derive(Debug, Display, Error, Diagnostic, From)]
#[non_exhaustive]
pub enum TarballError {
    #[diagnostic(code(ERR_PNPM_TARBALL_FETCH_TARBALL))]
    FetchTarball(NetworkError),

    /// The deployment's route policy refuses this origin. Only a server
    /// with an [`UpstreamRouteHook`](pnpm_network::UpstreamRouteHook)
    /// raises it: the CLI fetches as the user and reaches whatever the user
    /// configured.
    #[from(ignore)]
    #[display(
        "{url} is not allowed by this pnpr server; the operator must declare its registry as a public route or an upstream"
    )]
    #[diagnostic(code(ERR_PNPM_REGISTRY_OFF_ALLOWLIST))]
    OffAllowlist {
        #[error(not(source))]
        url: String,
    },

    #[diagnostic(code(ERR_PNPM_TARBALL_HTTP_STATUS))]
    HttpStatus(HttpStatusError),

    #[from(ignore)]
    #[diagnostic(code(ERR_PNPM_TARBALL_IO_ERROR))]
    ReadTarballEntries(std::io::Error),

    #[from(ignore)]
    #[display("Failed to read local tarball {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_TARBALL_READ_LOCAL_TARBALL))]
    ReadLocalTarball {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[diagnostic(
        code(ERR_PNPM_TARBALL_INTEGRITY),
        help(
            "The downloaded tarball does not match the integrity recorded in the lockfile. If you trust the new content (legitimate republish, or stale local metadata cache), run `pnpm install --update-checksums`. Otherwise treat this as a potential supply-chain issue and verify the new content first."
        )
    )]
    Checksum(VerifyChecksumError),

    #[from(ignore)]
    #[display("Failed to decode gzip: {_0}")]
    #[diagnostic(code(ERR_PNPM_TARBALL_DECODE_GZIP))]
    DecodeGzip(InflateDecodeErrors),

    /// The tarball's own `package.json` is not valid JSON. Only the
    /// resolve-time read ([`crate::local_tarball::read_local_tarball_metadata`]) raises this:
    /// there the manifest is the package's sole source of identity, so a
    /// corrupt one has to stop the install rather than degrade to an
    /// unnamed package. Matches the code and wording pnpm reports for
    /// the same tarball.
    #[from(ignore)]
    #[display("Failed to add tarball from \"{tarball}\" to store: {source}")]
    #[diagnostic(code(ERR_PNPM_TARBALL_EXTRACT))]
    ParseBundledManifest {
        tarball: String,
        #[error(source)]
        source: serde_json::Error,
    },

    #[from(ignore)]
    #[display("Failed to write cafs: {_0}")]
    #[diagnostic(transparent)]
    WriteCasFile(WriteCasFileError),

    #[from(ignore)]
    #[display("Failed to write store index (SQLite index): {_0}")]
    #[diagnostic(transparent)]
    WriteStoreIndex(StoreIndexError),

    #[from(ignore)]
    #[diagnostic(code(ERR_PNPM_TARBALL_TASK_JOIN_ERROR))]
    TaskJoin(tokio::task::JoinError),

    #[from(ignore)]
    #[display(
        "Archive at {url} advertised a Content-Length of {advertised_size} bytes, which exceeds what pnpm can allocate (either larger than `usize::MAX` on this target or memory pressure prevented a one-shot reservation)"
    )]
    #[diagnostic(code(ERR_PNPM_TARBALL_TOO_LARGE))]
    TarballTooLarge { url: String, advertised_size: u64 },

    /// A concurrent request for the same tarball URL went through
    /// `run_with_mem_cache`, drove the network fetch, and failed.
    /// This task was parked on the shared `Notify` waiting for the
    /// download; on wake it sees [`crate::CacheValue::Failed`] and surfaces
    /// this variant. The owner's original error stays with the
    /// owner (it can't be cloned past `reqwest::Error`).
    #[from(ignore)]
    #[display(
        "A concurrent fetch for {url} failed; this request waited on the shared mem cache and inherits the failure"
    )]
    #[diagnostic(code(ERR_PNPM_TARBALL_SIBLING_FETCH_FAILED))]
    SiblingFetchFailed { url: String },

    /// Path-traversal rejection on a zip entry, carrying the
    /// `PATH_TRAVERSAL` error code: any entry whose path is absolute
    /// or whose normalized form would land outside the target
    /// directory is rejected before any bytes are written to the CAS.
    #[from(ignore)]
    #[display("Refusing to extract zip entry {entry_path:?} from {url} — {reason}")]
    #[diagnostic(code(ERR_PNPM_PATH_TRAVERSAL))]
    PathTraversal { url: String, entry_path: String, reason: &'static str },

    /// Zip-archive parse / read error. Wraps the underlying `zip`
    /// crate error verbatim; pacquet does not interpret the failure
    /// mode beyond surfacing the entry path that triggered it.
    #[from(ignore)]
    #[display("Failed to read zip archive {url}: {source}")]
    #[diagnostic(code(ERR_PNPM_TARBALL_READ_ZIP))]
    ReadZipArchive {
        url: String,
        #[error(source)]
        source: zip::result::ZipError,
    },

    /// Per-entry I/O failure during zip extraction — `try_reserve`
    /// for the entry's payload, the body read, or any other
    /// [`std::io::Error`] surfaced from the zip iterator. Carries
    /// the archive URL and the entry path that triggered the
    /// failure so a corrupt archive is diagnosable from the user-
    /// facing message; the underlying [`std::io::Error`] is
    /// exposed as `source` for miette / `Error::source` walkers.
    /// Kept separate from [`TarballError::ReadTarballEntries`] so
    /// the retry-classification path emits `ERR_PNPM_ZIP`
    /// rather than the tar-specific `ERR_PNPM_TARBALL_TAR`.
    #[from(ignore)]
    #[display("Failed to read zip entry {entry_path:?} from {url}: {source}")]
    #[diagnostic(code(ERR_PNPM_TARBALL_READ_ZIP_ENTRY))]
    ReadZipEntries {
        url: String,
        entry_path: String,
        #[error(source)]
        source: std::io::Error,
    },

    /// `offline: true` was set and the package's tarball wasn't
    /// found in the local store. Pacquet refuses to fetch the
    /// network. pnpm's `--offline` only gates the metadata fetch;
    /// pacquet has no metadata fetch on the frozen-install path, so
    /// the same flag's most useful effect lands here: surface a
    /// clear "the snapshot isn't cached" error rather than letting
    /// the underlying network refusal propagate.
    ///
    /// `ERR_PNPM_NO_OFFLINE_TARBALL` is a pacquet-specific code;
    /// the message shape follows pnpm's `ERR_PNPM_NO_OFFLINE_META`
    /// — "Failed to resolve `<pkg>` in package mirror `<dir>`".
    #[from(ignore)]
    #[display(
        "Failed to fetch tarball for {package_id} from {url} in offline mode: snapshot not present in local store"
    )]
    #[diagnostic(
        code(ERR_PNPM_NO_OFFLINE_TARBALL),
        help(
            "Drop `--offline` (or `offline=true` in pnpm-workspace.yaml) or run an online install first to populate the store."
        )
    )]
    NoOfflineTarball { package_id: String, url: String },
}
