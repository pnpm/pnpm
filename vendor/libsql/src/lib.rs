//! # libSQL API for Rust
//!
//! libSQL is an embeddable SQL database engine based on SQLite.
//! This Rust API is a batteries-included wrapper around the SQLite C API to support transparent replication while retaining compatibility with the SQLite ecosystem, such as the SQL dialect and extensions. If you are building an application in Rust, this is the crate you should use.
//! There are also libSQL language bindings of this Rust crate to other languages such as [JavaScript](), Python, Go, and C.
//!
//! ## Getting Started
//!
//! To get started, you first need to create a [`Database`] object and then open a [`Connection`] to it, which you use to query:
//!
//! ```rust,no_run
//! # async fn run() {
//! use libsql::Builder;
//!
//! let db = Builder::new_local(":memory:").build().await.unwrap();
//! let conn = db.connect().unwrap();
//! conn.execute("CREATE TABLE IF NOT EXISTS users (email TEXT)", ()).await.unwrap();
//! conn.execute("INSERT INTO users (email) VALUES ('alice@example.org')", ()).await.unwrap();
//! # }
//! ```
//!
//! ## Embedded Replicas
//!
//! Embedded replica is libSQL database that's running in your application process, which keeps a local copy of a remote database.
//! They are useful if you want to move data in the memory space of your application for fast access.
//!
//! You can open an embedded read-only replica by using the [`Database::open_with_local_sync`] constructor:
//!
//! ```rust,no_run
//! # async fn run() {
//! use libsql::Builder;
//! use libsql::replication::Frames;
//!
//! let mut db = Builder::new_local_replica("/tmp/test.db").build().await.unwrap();
//!
//! let frames = Frames::Vec(vec![]);
//! db.sync_frames(frames).await.unwrap();
//! let conn = db.connect().unwrap();
//! conn.execute("SELECT * FROM users", ()).await.unwrap();
//! # }
//! ```
//!
//! ## Remote database
//!
//! It is also possible to create a libsql connection that does not open a local database but
//! instead sends queries to a remote database.
//!
//! ```rust,no_run
//! # async fn run() {
//! use libsql::Builder;
//!
//! let db = Builder::new_remote("libsql://my-remote-db.com".to_string(), "my-auth-token".to_string()).build().await.unwrap();
//! let conn = db.connect().unwrap();
//! conn.execute("CREATE TABLE IF NOT EXISTS users (email TEXT)", ()).await.unwrap();
//! conn.execute("INSERT INTO users (email) VALUES ('alice@example.org')", ()).await.unwrap();
//! # }
//! ```
//!
//! ## WASM
//!
//! Due to WASM requiring `!Send` support and the [`Database`] type supporting async and using
//! `async_trait` to abstract between the different database types, we are unable to support WASM
//! via the [`Database`] type. Instead, we have provided simpler parallel types in the `wasm`
//! module that provide access to our remote HTTP protocol in WASM.
//!
//! ## Examples
//!
//! You can find more examples in the [`examples`](https://github.com/tursodatabase/libsql/tree/main/libsql/examples) directory.
//!
//! ## Feature flags
//!
//! This crate provides a few feature flags that will help you improve compile times by allowing
//! you to reduce the dependencies needed to compile specific features of this crate. For example,
//! you may not want to compile the libsql C code if you just want to make HTTP requests. Feature
//! flags may be used by including the libsql crate like:
//!
//! ```toml
//! libsql = { version = "*", default-features = false, features = ["core", "replication", "remote" ]
//! ```
//!
//! By default, all the features are enabled but by providing `default-features = false` it will
//! remove those defaults.
//!
//! The features are descirbed like so:
//! - `core` this includes the core C code that backs both the basic local database usage and
//! embedded replica features.
//! - `replication` this feature flag includes the `core` feature flag and adds on top HTTP code
//! that will allow you to sync you remote database locally.
//! - `remote` this feature flag only includes HTTP code that will allow you to run queries against
//! a remote database.
//! - `tls` this feature flag disables the builtin TLS connector and instead requires that you pass
//! your own connector for any of the features that require HTTP.

// pnpm: the workspace builds with `-D warnings`, which reaches path dependencies.
#![allow(warnings)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(
    all(
        any(
            not(feature = "remote"),
            not(feature = "replication"),
            not(feature = "core")
        ),
        feature = "tls"
    ),
    allow(unused_imports)
)]
#![cfg_attr(
    all(
        any(
            not(feature = "remote"),
            not(feature = "replication"),
            not(feature = "core")
        ),
        feature = "tls"
    ),
    allow(dead_code)
)]

#[macro_use]
mod macros;

cfg_core! {
    mod local;

    pub use local::{version, version_number, RowsFuture};
    pub use database::OpenFlags;

    pub use database::{Cipher, EncryptionConfig};
}

pub mod params;

cfg_sync! {
    mod sync;
    pub use database::SyncProtocol;
    pub use database::EncryptionContext;
    pub use database::EncryptionKey;
}

cfg_replication! {
    pub mod replication;
}

cfg_core! {
    pub use libsql_sys::ffi;
}

cfg_wasm! {
    pub mod wasm;
}

mod util;

pub mod errors;
pub use errors::Error;

pub use params::params_from_iter;

mod auth;
mod connection;
mod database;
mod load_extension_guard;

cfg_parser! {
    mod parser;
}

mod rows;
mod statement;
mod transaction;
mod value;

#[cfg(feature = "serde")]
pub mod de;

pub use value::{Value, ValueRef, ValueType};

cfg_hrana! {
    mod hrana;
}

pub use self::{
    auth::{AuthAction, AuthContext, Authorization},
    connection::{AuthHook, BatchRows, Connection, Op},
    database::{Builder, Database},
    load_extension_guard::LoadExtensionGuard,
    rows::{Column, Row, Rows},
    statement::Statement,
    transaction::{Transaction, TransactionBehavior},
};

/// Convenient alias for `Result` using the `libsql::Error` type.
pub type Result<T> = std::result::Result<T, errors::Error>;
pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;
