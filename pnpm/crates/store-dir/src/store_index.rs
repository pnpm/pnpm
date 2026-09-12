pub use writer::StoreIndexWriter;

use crate::StoreDir;
use derive_more::{Display, Error};
use miette::Diagnostic;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use url::Url;

/// SQLite-backed per-package index that pnpm v11 stores alongside the CAFS
/// blobs. In the pacquet layout the file lives at
/// `<store-root>/v11/index.db` — call [`StoreIndex::open_in`] with a
/// [`StoreDir`] to hit that path, or [`StoreIndex::open`] with any directory
/// to drop `index.db` right inside it (used by tests and tools).
///
/// Each row keys a package by its tarball integrity plus a package identifier
/// and stores a msgpack-encoded [`PackageFilesIndex`]. The schema and PRAGMAs
/// below match pnpm v11's on-disk index format so that the two tools can read
/// each other's entries.
pub struct StoreIndex {
    conn: Connection,
}

/// Shared handle to a read-only [`StoreIndex`] that can be cheaply cloned and
/// sent across blocking tasks. `SQLite`'s `Connection` is `Send` but not
/// `Sync`, so the `Mutex` gates concurrent reads to a single query at a time
/// — fine for our workload where every caller serializes one short query and
/// then hands off to per-file work without holding the lock.
pub type SharedReadonlyStoreIndex = Arc<Mutex<StoreIndex>>;

/// Error type of [`StoreIndex`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StoreIndexError {
    #[display("Failed to create directory for index.db at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_CREATE_DIR))]
    CreateDir {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to open index.db at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_OPEN))]
    Open {
        path: PathBuf,
        #[error(source)]
        source: rusqlite::Error,
    },

    /// The store path could not be turned into the `file:` URI that the
    /// immutable open requires (see [`StoreIndex::open_immutable`]).
    #[display("Failed to build a file: URI for index.db at {path:?}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_FILE_URI))]
    FileUri {
        path: PathBuf,
        #[error(source)]
        source: Option<std::io::Error>,
    },

    #[display("Failed to initialize index.db schema: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_INIT_SCHEMA))]
    InitSchema {
        #[error(source)]
        source: rusqlite::Error,
    },

    #[display("Failed to read from index.db: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_READ))]
    Read {
        #[error(source)]
        source: rusqlite::Error,
    },

    #[display("Failed to write to index.db: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_WRITE))]
    Write {
        #[error(source)]
        source: rusqlite::Error,
    },

    #[display("Failed to encode PackageFilesIndex as msgpackr records: {source}")]
    #[diagnostic(transparent)]
    Encode {
        #[error(source)]
        source: crate::msgpackr_records::EncodeError,
    },

    #[display("Failed to decode PackageFilesIndex from msgpack: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_DECODE))]
    Decode {
        #[error(source)]
        source: rmp_serde::decode::Error,
    },

    #[display("Failed to transcode msgpackr-records payload to plain msgpack: {source}")]
    #[diagnostic(transparent)]
    Transcode {
        #[error(source)]
        source: crate::msgpackr_records::DecodeError,
    },
}

impl StoreIndex {
    /// Open (or create) the `index.db` under `store_dir` and configure the
    /// same PRAGMAs pnpm v11 uses.
    pub fn open(store_dir: &Path) -> Result<Self, StoreIndexError> {
        std::fs::create_dir_all(store_dir).map_err(|source| StoreIndexError::CreateDir {
            path: store_dir.to_path_buf(),
            source,
        })?;
        let db_path = store_dir.join("index.db");
        let conn = Connection::open(&db_path)
            .map_err(|source| StoreIndexError::Open { path: db_path, source })?;

        // Busy-timeout FIRST so the internal busy handler is active during the
        // rest of the setup — on Windows file locking is mandatory and
        // concurrent pacquet / pnpm invocations can contend.
        conn.execute_batch(
            "
            PRAGMA busy_timeout=5000;
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA mmap_size=536870912;
            PRAGMA cache_size=-32000;
            PRAGMA temp_store=MEMORY;
            PRAGMA wal_autocheckpoint=10000;
            CREATE TABLE IF NOT EXISTS package_index (
              key TEXT PRIMARY KEY,
              data BLOB NOT NULL
            ) WITHOUT ROWID;
            ",
        )
        .map_err(|source| StoreIndexError::InitSchema { source })?;

        Ok(StoreIndex { conn })
    }

    /// Open the `index.db` that lives directly under a [`StoreDir`]'s root.
    pub fn open_in(store_dir: &StoreDir) -> Result<Self, StoreIndexError> {
        StoreIndex::open(store_dir.root())
    }

    /// Open an existing `index.db` read-only. Skips the schema-mutating
    /// PRAGMAs (`journal_mode=WAL`, `synchronous`, `wal_autocheckpoint`)
    /// and `CREATE TABLE IF NOT EXISTS`. The connection still participates
    /// in WAL locking (it may create the `-shm` sidecar in the store
    /// directory), so it stays consistent while a concurrent writer — this
    /// process's [`StoreIndexWriter`] or another pnpm/pacquet process —
    /// mutates the same database. For a store on a read-only filesystem use
    /// [`StoreIndex::open_immutable`] instead.
    ///
    /// We *do* set `busy_timeout`: it's a connection-local wait, not a
    /// DB mutation, and without it a concurrent writer (pnpm or another
    /// pacquet process) turns every cache lookup during contention into
    /// an immediate `SQLITE_BUSY` — i.e. a spurious cache miss that
    /// triggers a full re-download. 5 s matches the writer side.
    pub fn open_readonly(store_dir: &Path) -> Result<Self, StoreIndexError> {
        let db_path = store_dir.join("index.db");
        let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|source| StoreIndexError::Open { path: db_path.clone(), source })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|source| StoreIndexError::Open { path: db_path, source })?;
        Ok(StoreIndex { conn })
    }

    /// Open an existing `index.db` from a store that is complete and
    /// read-only (`frozenStore`): a Nix store, a read-only bind mount, an
    /// OCI layer. `index.db` is a WAL-mode database, and even a plain
    /// `SQLITE_OPEN_READ_ONLY` open needs to create the `-shm` sidecar in
    /// the store directory — which fails with "attempt to write a readonly
    /// database" when the directory is not writable. Opening through the
    /// `file:…?immutable=1` URI tells `SQLite` the file cannot change
    /// underneath it, so it bypasses the WAL/shm machinery entirely and
    /// reads the raw file with zero sidecar creation.
    ///
    /// The immutability assertion is load-bearing: `SQLite` skips all locking
    /// and change detection on this connection, so a concurrent writer makes
    /// reads undefined (stale or corrupt results). Only open this way under
    /// `frozenStore`, which disables every store-write path — for a store
    /// that other connections may write, use [`StoreIndex::open_readonly`].
    /// No `busy_timeout` is set because an immutable connection takes no
    /// locks for it to wait on.
    pub fn open_immutable(store_dir: &Path) -> Result<Self, StoreIndexError> {
        let db_path = store_dir.join("index.db");
        let uri = immutable_sqlite_uri(&db_path)?;
        let conn = Connection::open_with_flags(
            &uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|source| StoreIndexError::Open { path: db_path, source })?;
        Ok(StoreIndex { conn })
    }

    /// Read-only counterpart to [`StoreIndex::open_in`].
    pub fn open_readonly_in(store_dir: &StoreDir) -> Result<Self, StoreIndexError> {
        StoreIndex::open_readonly(store_dir.root())
    }

    /// Open a read-only index wrapped in `Arc<Mutex<…>>` so it can be shared
    /// across the many cache lookups an install performs. Returns `None` if
    /// `index.db` does not yet exist under the store — a first-time install
    /// against an empty store — since there is nothing to read back and every
    /// lookup would be a miss anyway.
    ///
    /// Reusing one connection avoids reopening the `SQLite` database (and
    /// redoing its PRAGMAs) on every package, which otherwise scales
    /// linearly with the snapshot count.
    pub fn shared_readonly_in(store_dir: &StoreDir) -> Option<SharedReadonlyStoreIndex> {
        StoreIndex::shared_in(store_dir, StoreIndex::open_readonly)
    }

    /// [`StoreIndex::shared_readonly_in`] for a frozen store — opens via
    /// [`StoreIndex::open_immutable`], with its no-concurrent-writer
    /// contract.
    pub fn shared_immutable_in(store_dir: &StoreDir) -> Option<SharedReadonlyStoreIndex> {
        StoreIndex::shared_in(store_dir, StoreIndex::open_immutable)
    }

    /// Open the shared read-only index the way `frozenStore` requires:
    /// [`StoreIndex::open_immutable`] when frozen, so no WAL or SHM
    /// sidecar is created under a read-only store root, and
    /// [`StoreIndex::open_readonly`] otherwise.
    ///
    /// `None` means the store has no `index.db` yet — a first install
    /// against an empty store — and every lookup simply misses.
    #[must_use]
    pub fn shared_for(
        store_dir: &StoreDir,
        frozen_store: bool,
    ) -> Option<SharedReadonlyStoreIndex> {
        if frozen_store {
            StoreIndex::shared_immutable_in(store_dir)
        } else {
            StoreIndex::shared_readonly_in(store_dir)
        }
    }

    /// [`StoreIndex::shared_for`] off the reactor thread.
    ///
    /// Opening is synchronous `SQLite` I/O (`Connection::open_with_flags`
    /// plus a `PRAGMA busy_timeout`), so it is parked on the blocking
    /// pool even for the sub-millisecond it usually takes.
    ///
    /// A join failure — a panic in the blocking task, or cancellation
    /// during runtime shutdown — degrades to `None` rather than failing
    /// the caller: every lookup then misses, which callers already
    /// handle for the empty-store case. It is surfaced at `warn!` so a
    /// silent panic stays diagnosable.
    pub async fn open_shared(
        store_dir: &'static StoreDir,
        frozen_store: bool,
    ) -> Option<SharedReadonlyStoreIndex> {
        match tokio::task::spawn_blocking(move || StoreIndex::shared_for(store_dir, frozen_store))
            .await
        {
            Ok(index) => index,
            Err(error) => {
                tracing::warn!(
                    target: "pacquet::store_index",
                    ?error,
                    "store-index open task failed; continuing without a shared cache index",
                );
                None
            }
        }
    }

    fn shared_in(
        store_dir: &StoreDir,
        open: fn(&Path) -> Result<Self, StoreIndexError>,
    ) -> Option<SharedReadonlyStoreIndex> {
        let store_root = store_dir.root();
        if !store_root.join("index.db").exists() {
            return None;
        }
        open(store_root).ok().map(|index| Arc::new(Mutex::new(index)))
    }
}

/// Decode one `package_index.data` blob into a [`PackageFilesIndex`].
/// Exposed publicly so callers reading raw rows via
/// [`StoreIndex::get_many_raw`] can run the decode outside the
/// store-index mutex (typically across a rayon pool — the decode
/// is the dominant CPU cost for rows that carry a `manifest`
/// field).
pub fn decode_package_files_index(bytes: &[u8]) -> Result<PackageFilesIndex, StoreIndexError> {
    decode_index_value(bytes)
}

fn decode_index_value(bytes: &[u8]) -> Result<PackageFilesIndex, StoreIndexError> {
    // `transcode_to_plain_msgpack` tracks records-mode internally and
    // only reinterprets `0x40..=0x7f` as slot references after a record
    // definition has been observed, so it's safe to run on both
    // pacquet-written (plain msgpack) and pnpm-written (msgpackr records)
    // rows. For plain rows it still performs the integer-valued float
    // narrowing we need on the read side — pacquet writes the
    // `checkedAt` timestamp as `float 64` for JS/BigInt interop.
    let plain = crate::msgpackr_records::transcode_to_plain_msgpack(bytes)
        .map_err(|source| StoreIndexError::Transcode { source })?;
    rmp_serde::from_slice(&plain).map_err(|source| StoreIndexError::Decode { source })
}

/// Build the `SQLite` key pnpm uses: `"{integrity}\t{pkg_id}"`. Integrity strings
/// never contain tabs so the separator is unambiguous.
#[must_use]
pub fn store_index_key(integrity: &str, pkg_id: &str) -> String {
    format!("{integrity}\t{pkg_id}")
}

/// Store-index key for git-hosted tarballs and bare `type: git` resolutions.
///
/// The cached content of a git-hosted package depends on whether build scripts
/// ran during fetch (`preparePackage`), so `built` is part of the key. The
/// integrity-only key would collapse the built/not-built variants into one
/// slot.
#[must_use]
pub fn git_hosted_store_index_key(pkg_id: &str, built: bool) -> String {
    store_index_key(pkg_id, if built { "built" } else { "not-built" })
}

/// Pick the store-index key for a tarball-shaped resolution.
///
/// The `built` flag must match the build decision the caller will make at
/// fetch time (the negation of whether build scripts are ignored).
#[must_use]
pub fn pick_store_index_key(
    integrity: Option<&str>,
    git_hosted: bool,
    pkg_id: &str,
    built: bool,
) -> String {
    match integrity {
        Some(integrity) if !git_hosted => store_index_key(integrity, pkg_id),
        _ => git_hosted_store_index_key(pkg_id, built),
    }
}

/// Per-instance record of what a tarball contributed to the CAFS. Stored as the
/// value half of each `package_index` row.
///
/// Shaped to interop with pnpm v11's package-files index so the two tools
/// can share `index.db`.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFilesIndex {
    /// Subset of the tarball's `package.json` that both tools keep on hand to
    /// avoid re-reading the manifest for each install. Also the row's second
    /// statement of which package it holds — see [`crate::pkg_content_mismatch`].
    /// `None` for a row written before either tool kept it, and for a tarball
    /// whose `package.json` failed to parse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<serde_json::Value>,

    /// Whether the package's lifecycle scripts demand a build step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_build: Option<bool>,

    /// Whether fetching the git package required running preparation scripts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_prepare: Option<bool>,

    /// The digest algorithm used for every `files.*.digest` entry, e.g. `sha512`.
    pub algo: String,

    /// Map of in-tarball path → CAFS file metadata.
    pub files: HashMap<String, CafsFileInfo>,

    /// Side-effect overlays applied after post-install scripts. Populated
    /// by the build-side-effects cache (WRITE path).
    ///
    /// Pacquet's on-disk byte stability for this field comes from
    /// the msgpackr-records encoder
    /// (`crate::msgpackr_records::encode_package_files_index`),
    /// which iterates the map in sorted-key order before emitting.
    /// The serde `Serialize` impl below is never reached on the
    /// write path — `StoreIndex::set_many` always routes through
    /// the bespoke encoder — but is kept for the read path's
    /// round-trip through `rmp_serde::from_slice`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_effects: Option<HashMap<String, SideEffectsDiff>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_side_effects_quarantine: Option<HashMap<String, Vec<String>>>,
}

/// Value of [`PackageFilesIndex::files`]. Matches pnpm v11's per-file
/// metadata field-for-field so that the msgpack payload interops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CafsFileInfo {
    /// Content-addressed digest of the file — raw hex (no `sha512-` prefix),
    /// matching pnpm v11's `digest` field in the cafs index.
    pub digest: String,
    pub mode: u32,
    pub size: u64,
    /// Millisecond Unix timestamp of the last integrity check, or `None`
    /// if never verified.
    ///
    /// Wire note: serialized as `MessagePack` `float 64` so the byte
    /// encoding matches what pnpm itself emits (JS `Number` is a double,
    /// so msgpackr writes timestamps past int32 range as `cb` + 8
    /// bytes). Writing as `uint 64` instead would be "correct" `MessagePack`
    /// but msgpackr would decode it as a `BigInt`, and pnpm's integrity
    /// check does `mtimeMs - (checkedAt ?? 0)` — mixing Number and
    /// `BigInt` throws `TypeError` at runtime. On the read side, the
    /// [`transcode_to_plain_msgpack`][crate::msgpackr_records::transcode_to_plain_msgpack]
    /// step narrows integer-valued floats back to `uint 64` so
    /// `rmp_serde` can deserialize into `Option<u64>` without complaint.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "serialize_checked_at")]
    pub checked_at: Option<u64>,
}

/// Emit `Option<u64>` on the msgpack wire as `float 64` rather than
/// `uint 64`. See the doc on [`CafsFileInfo::checked_at`] for the
/// interop reasoning — short version, msgpackr reads `uint 64` as a
/// `BigInt` and pnpm's integrity check then crashes on Number/BigInt
/// mixing.
#[expect(clippy::ref_option, reason = "serde serialize_with is invoked as f(&field, serializer)")]
fn serialize_checked_at<Serializer: serde::Serializer>(
    value: &Option<u64>,
    serializer: Serializer,
) -> Result<Serializer::Ok, Serializer::Error> {
    match value {
        Some(inner) => serializer.serialize_f64(*inner as f64),
        None => serializer.serialize_none(),
    }
}

/// Value of [`PackageFilesIndex::side_effects`].
///
/// Byte stability of the `added` field on the wire is provided by
/// the bespoke encoder in `msgpackr_records.rs` (it iterates the
/// map in sorted-key order). The derived serde `Serialize` impl
/// is unused on the write path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SideEffectsDiff {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<HashMap<String, CafsFileInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_origin: Option<RemoteSideEffectsOrigin>,
}

impl SideEffectsDiff {
    /// Whether the diff names no added and no deleted files.
    ///
    /// A diff is recorded whenever a build script ran, so a build whose
    /// whole effect lands outside the package directory (a git-hook
    /// installer, a shared download cache) records an empty one.
    /// Restoring it materializes nothing, so treating it as a cache hit
    /// would skip the scripts and put nothing in their place. Readers
    /// drop such a diff and rebuild, as pnpm 11 does, and publishers do
    /// not share it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.as_ref().is_none_or(HashMap::is_empty)
            && self.deleted.as_ref().is_none_or(Vec::is_empty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSideEffectsOrigin {
    pub channel: String,
    pub owner: pnpm_shared_artifact_protocol::OwnerScope,
    pub signer_key_id: String,
    pub builder_profile: pnpm_shared_artifact_protocol::BuilderProfile,
    pub envelope: pnpm_shared_artifact_protocol::SignedArtifactEnvelope,
    pub verification: String,
}

/// Build the `file://…?immutable=1` URI used to open `index.db` read-only (see
/// [`StoreIndex::open_immutable`] for why immutable). [`Url::from_file_path`]
/// yields a canonical file URL on every platform: it percent-encodes the URI
/// delimiters that would otherwise truncate the path or inject a query/fragment
/// (`?`, `#`, `%`, spaces) and, on Windows, maps the drive letter and
/// backslashes into a valid `file:///C:/…` form. See <https://sqlite.org/uri.html>.
///
/// [`Url::from_file_path`] only accepts an absolute path, so a relative store
/// path is first absolutized against the current directory — the same
/// resolution Node's `pathToFileURL` applies on the pnpm side.
fn immutable_sqlite_uri(db_path: &Path) -> Result<String, StoreIndexError> {
    let absolute = std::path::absolute(db_path).map_err(|source| StoreIndexError::FileUri {
        path: db_path.to_path_buf(),
        source: Some(source),
    })?;
    let mut url = Url::from_file_path(&absolute)
        .map_err(|()| StoreIndexError::FileUri { path: absolute, source: None })?;
    url.query_pairs_mut().append_pair("immutable", "1");
    Ok(url.into())
}

#[cfg(test)]
mod tests;

mod writer;

mod queries;
