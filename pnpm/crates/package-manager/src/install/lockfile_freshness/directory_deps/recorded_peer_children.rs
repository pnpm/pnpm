use pnpm_lockfile::{BoundProviders, Lockfile, PackageKey, PkgName, SnapshotEntry};
use std::collections::{HashMap, HashSet};

/// Aliases supplied as peers by a reachable occurrence of a recorded directory.
#[derive(Default)]
pub(crate) struct RecordedPeerChildren {
    aliases: std::sync::OnceLock<Option<HashMap<PackageKey, HashSet<PkgName>>>>,
}

impl RecordedPeerChildren {
    pub(super) fn contains(&self, lockfile: &Lockfile, key: &PackageKey, alias: &str) -> bool {
        self.aliases
            .get_or_init(|| collect(lockfile))
            .as_ref()
            .and_then(|aliases| aliases.get(&key.without_peer()))
            .is_some_and(|aliases| {
                alias
                    .parse::<PkgName>()
                    .is_ok_and(|name| aliases.contains(&name))
            })
    }
}

fn collect(lockfile: &Lockfile) -> Option<HashMap<PackageKey, HashSet<PkgName>>> {
    let reachable = lockfile.reachable_snapshot_keys().ok()?;
    let snapshots = lockfile.snapshots.as_ref()?;
    let packages = lockfile.packages.as_ref()?;
    let mut aliases = HashMap::<_, HashSet<_>>::new();
    for key in reachable {
        let bare = key.without_peer();
        let Some(metadata) = packages.get(&bare).filter(|entry| entry.specifiers.is_some()) else {
            continue;
        };
        let Some(peers) = &metadata.peer_dependencies else { continue };
        let bound = BoundProviders::from_recorded_snapshot(lockfile.into(), &key);
        if bound.is_hashed() {
            continue;
        }
        aliases
            .entry(bare)
            .or_default()
            .extend(bound_children(&snapshots[&key], peers, &bound));
    }
    Some(aliases)
}

fn bound_children(
    snapshot: &SnapshotEntry,
    peers: &HashMap<String, String>,
    bound: &BoundProviders,
) -> HashSet<PkgName> {
    peers
        .keys()
        .filter_map(|alias| alias.parse().ok())
        .filter(|alias| {
            [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
                .into_iter()
                .flatten()
                .filter_map(|children| children.get(alias))
                .filter_map(|reference| reference.resolve(alias))
                .any(|provider| bound.binds(&provider))
        })
        .collect()
}
