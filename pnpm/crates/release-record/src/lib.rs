#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

//! The trust primitives of pnpr release records, shared by the registry that
//! produces them and the clients that verify them.
//!
//! - [`record`]: a record's core fields, read from the exact bytes the
//!   registry stored, and its [`RecordDigest`], the SHA-512 of those bytes.
//! - [`purl`]: the Package URL of an npm, Cargo, Python, or OCI record.
//! - [`identity`]: what makes two records the same record, the log entry
//!   that commits to one, and the cache key that keeps records of different
//!   registries apart.
//! - [`statement`]: in-toto Statements in DSSE envelopes, the acceptance,
//!   publication, and tag binding claims a registry signs about a record.
//! - [`tuf`] and [`policy`]: the TUF repository that authenticates a
//!   registry's `pnpr-log-policy.json`, and the policy it carries: the log,
//!   its checkpoint and statement keys, and its witnesses.

pub mod digest;
pub mod dsse;
pub mod identity;
pub mod policy;
pub mod profile;
pub mod purl;
pub mod record;
pub mod statement;
pub mod tuf;

pub use digest::*;
pub use dsse::*;
pub use identity::*;
pub use policy::*;
pub use profile::*;
pub use purl::*;
pub use record::*;
pub use statement::*;

#[cfg(test)]
mod test_support;
