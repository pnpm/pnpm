use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::PathBuf;

/// Error type of [`StoreIndex`](super::StoreIndex).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StoreIndexError {
    #[cfg(target_os = "wasi")]
    #[display("Failed to coordinate access to index.db at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_INDEX_LEASE))]
    Lease {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Store-index writer is unavailable")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_WRITER_UNAVAILABLE))]
    WriterUnavailable,

    #[display("Failed to create the store-index directory at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_CREATE_DIR))]
    CreateDir {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to create index.db at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_CREATE_FILE))]
    CreateFile {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to create a store-index entry: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_CREATE_ENTRY))]
    CreateEntry {
        #[error(source)]
        source: pnpm_fs::EnsureFileError,
    },

    #[display("Failed to write the store-index entry at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_WRITE_ENTRY))]
    WriteEntry {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to read the store index at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_STORE_INDEX_READ_ENTRY))]
    ReadEntry {
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
    /// immutable open requires (see [`StoreIndex::open_immutable`](super::StoreIndex::open_immutable)).
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
