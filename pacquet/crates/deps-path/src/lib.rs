//! Pacquet port of pnpm's
//! [`@pnpm/deps.path`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts).
//!
//! String manipulation over depPaths (`name@version(peer1@v)(peer2@v)`).
//! Pacquet already carries typed parsers for the simple shapes
//! (`pacquet_lockfile::PkgNameVerPeer` et al. — referenced as plain
//! text because this crate deliberately doesn't depend on
//! `pacquet-lockfile`), but the peer-resolution stage needs a handful
//! of pure string helpers that operate on the pre-typed surface: build
//! a peer suffix from a list of peer IDs, turn a depPath into a
//! filesystem-safe directory name (with the length cap that the typed
//! `to_virtual_store_name` shortcut skips), and walk balanced parens
//! to locate the peer-suffix / `(patch_hash=…)` boundary.

mod create_peer_dep_graph_hash;
mod dep_path;
mod dep_path_to_filename;
mod is_runtime_dep_path;
mod link_path_to_peer_version;
mod peer_id;
mod suffix_index;
mod try_get_package_id;

pub use create_peer_dep_graph_hash::create_peer_dep_graph_hash;
pub use dep_path::DepPath;
pub use dep_path_to_filename::dep_path_to_filename;
pub use is_runtime_dep_path::is_runtime_dep_path;
pub use link_path_to_peer_version::link_path_to_peer_version;
pub use peer_id::PeerId;
pub use suffix_index::{
    DepPathSuffixIndex, get_pkg_id_with_patch_hash, index_of_dep_path_suffix, remove_suffix,
};
pub use try_get_package_id::try_get_package_id;
