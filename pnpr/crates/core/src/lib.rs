//! The bottom layer of [pnpr](https://github.com/pnpm/pnpm): the error type
//! every other crate returns, package-name parsing, the registry routing
//! table, and the access policy those routes are gated by.
//!
//! Nothing here talks to the network, the filesystem, or an object store —
//! that is what makes it the layer everything else can depend on.

pub mod error;
pub mod package_name;
pub mod policy;
pub mod registry;
