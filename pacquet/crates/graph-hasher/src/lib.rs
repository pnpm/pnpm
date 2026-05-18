//! Pacquet port of pnpm's `@pnpm/crypto.object-hasher` (which itself
//! wraps npm's `object-hash` library) and `@pnpm/deps.graph-hasher`.
//!
//! The cache key used by the side-effects cache is **on disk and
//! shared with pnpm**, so the hash output must be byte-for-byte
//! identical to what pnpm produces. The `object-hash` library uses a
//! specific recursive bytestream format under the hood
//! (`object:<N>:<key>:<value>,...` with sorted keys, `string:<utf16_len>:<value>`,
//! etc.) — pacquet replicates that format here.
//!
//! References (pinned to b4f8f47ac2 / object-hash@3.0.0):
//! - <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/crypto/object-hasher/src/index.ts>
//! - <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts>
//! - <https://github.com/puleos/object-hash/blob/v3.0.0/index.js>

mod dep_state;
mod engine_name;
mod global_virtual_store_path;
mod object_hasher;

pub use dep_state::{CalcDepStateOptions, DepsGraphNode, DepsStateCache, calc_dep_state};
pub use engine_name::{
    detect_node_major, detect_node_version, engine_name, host_arch, host_libc, host_platform,
};
pub use global_virtual_store_path::{calc_graph_node_hash, format_global_virtual_store_path};
pub use object_hasher::{hash_object, hash_object_with_encoding, hash_object_without_sorting};

/// Hex/base64 encoding option for [`hash_object_with_encoding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashEncoding {
    Base64,
    Hex,
}

#[cfg(test)]
mod tests;
