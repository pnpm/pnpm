use crate::dep_path::DepPath;

/// One peer entry fed to [`fn@crate::create_peer_dep_graph_hash`]. Mirrors
/// pnpm's
/// [`PeerId`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L195)
/// union: either `{ name, version }` or a pre-rendered depPath.
///
/// `Pair` is used when the peer resolver wants to identify a peer by its
/// `name@version` (the common path, and what `dedupePeers` emits).
/// `DepPath` carries an already-built depPath — for transitive peers
/// that have their own peers, the suffix is recursive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PeerId {
    Pair { name: String, version: String },
    DepPath(DepPath),
}

impl PeerId {
    /// String form used inside the joined peer suffix. Pair entries are
    /// `name@version`; depPaths are passed through with a single
    /// leading `/` stripped (mirrors upstream's `peerId[0] === '/'`
    /// fast path for the absolute / relative depPath distinction).
    #[must_use]
    pub fn as_segment(&self) -> String {
        match self {
            PeerId::Pair { name, version } => format!("{name}@{version}"),
            PeerId::DepPath(dep_path) => match dep_path.as_str().strip_prefix('/') {
                Some(rest) => rest.to_string(),
                None => dep_path.as_str().to_string(),
            },
        }
    }
}
