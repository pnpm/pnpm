//! Resolves `node@runtime:<spec>` dependencies against the Node.js
//! release index. The bare specifier carries a release channel
//! (`release`, `nightly`, `rc`, `test`, `v8-canary`) plus a version
//! selector that may be a semver range, an exact version, a dist tag
//! (`lts`, `latest`), or one of the LTS codenames (`argon`, `iron`,
//! ...). Once a concrete version is picked, the resolver crawls the
//! mirror's `SHASUMS256.txt` to enumerate every platform-specific
//! artifact and emits one
//! [`VariationsResolution`](pnpm_lockfile::VariationsResolution)
//! variant per `(os, cpu, libc?)` triple.

#[cfg(target_family = "wasm")]
extern crate pnpm_http as reqwest;

pub use get_node_artifact_address::{
    GetNodeArtifactAddressOptions, NodeArtifactAddress, get_node_artifact_address,
};
pub use get_node_mirror::{
    DEFAULT_NODE_MIRROR_BASE_URL, UNOFFICIAL_NODE_MIRROR_BASE_URL, get_node_mirror,
};
pub use node_resolver::{
    NodeResolver, NodeResolverError, normalize_node_runtime_version_specifier,
};
pub use normalize_arch::get_normalized_arch;
pub use parse_node_specifier::{NodeSpecifier, ParseNodeSpecifierError, parse_node_specifier};
pub use resolve_node_version::{
    NODE_EXTRAS_IGNORE_PATTERN, ResolveNodeVersionError, resolve_node_version,
    resolve_node_version_with_auth, resolve_node_versions, resolve_node_versions_with_auth,
};

mod get_node_artifact_address;
mod get_node_mirror;
mod node_resolver;
mod normalize_arch;
mod parse_node_specifier;
mod resolve_node_version;
