//! Pacquet port of pnpm's
//! [`@pnpm/engine.runtime.node-resolver`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts).
//!
//! Resolves `node@runtime:<spec>` dependencies against the Node.js
//! release index. The bare specifier carries a release channel
//! (`release`, `nightly`, `rc`, `test`, `v8-canary`) plus a version
//! selector that may be a semver range, an exact version, a dist tag
//! (`lts`, `latest`), or one of the LTS codenames (`argon`, `iron`,
//! ...). Once a concrete version is picked, the resolver crawls the
//! mirror's `SHASUMS256.txt` to enumerate every platform-specific
//! artifact and emits one
//! [`VariationsResolution`](pacquet_lockfile::VariationsResolution)
//! variant per `(os, cpu, libc?)` triple.
//!
//! Three pieces:
//!
//! - [`parse_node_specifier()`] — recognise the `<channel>/<spec>`,
//!   `<channel>`, prerelease, alias, and bare-range forms. Ports
//!   [`parseNodeSpecifier.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/parseNodeSpecifier.ts).
//! - [`get_node_mirror()`] / [`get_node_artifact_address()`] /
//!   [`get_normalized_arch`] — mirror URL normalisation, archive URL
//!   composition, and the arch quirks for ia32 Windows / armv7l Linux
//!   / Apple-Silicon-on-pre-16 macOS. Port
//!   [`getNodeMirror.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/getNodeMirror.ts),
//!   [`getNodeArtifactAddress.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/getNodeArtifactAddress.ts),
//!   and
//!   [`normalizeArch.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/normalizeArch.ts).
//! - [`NodeResolver`] — the [`Resolver`](pacquet_resolving_resolver_base::Resolver)
//!   impl that ties the parser, mirror config, and asset-list fetch
//!   into the dispatcher chain. Ports `index.ts` in the same file
//!   tree.

mod get_node_artifact_address;
mod get_node_mirror;
mod node_resolver;
mod normalize_arch;
mod parse_node_specifier;
mod resolve_node_version;

pub use get_node_artifact_address::{
    GetNodeArtifactAddressOptions, NodeArtifactAddress, get_node_artifact_address,
};
pub use get_node_mirror::{
    DEFAULT_NODE_MIRROR_BASE_URL, UNOFFICIAL_NODE_MIRROR_BASE_URL, get_node_mirror,
};
pub use node_resolver::{NodeResolver, NodeResolverError};
pub use normalize_arch::get_normalized_arch;
pub use parse_node_specifier::{NodeSpecifier, ParseNodeSpecifierError, parse_node_specifier};
pub use resolve_node_version::{
    NODE_EXTRAS_IGNORE_PATTERN, ResolveNodeVersionError, resolve_node_version,
    resolve_node_versions,
};
