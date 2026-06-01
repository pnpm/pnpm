use super::find_own_runtime_node_major;
use pacquet_lockfile::{PkgName, SnapshotDepRef, SnapshotEntry};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

/// `dependencies.node: 'runtime:<v>'` is the desugared form pnpm's
/// resolver writes when a dep declares its own `engines.runtime`
/// (see [`installing/deps-resolver/src/resolveDependencies.ts:1477-1479`](https://github.com/pnpm/pnpm/blob/29a42efc3b/installing/deps-resolver/src/resolveDependencies.ts#L1477-L1479)).
/// The helper pulls the bare major back out.
#[test]
fn picks_up_runtime_pin_from_dependencies() {
    let mut deps = HashMap::new();
    deps.insert(
        PkgName::parse("node").expect("parse pkg name"),
        SnapshotDepRef::Plain("runtime:22.11.0".parse().expect("parse ver-peer")),
    );
    let snapshot = SnapshotEntry { dependencies: Some(deps), ..SnapshotEntry::default() };
    assert_eq!(find_own_runtime_node_major(&snapshot), Some(22));
}

/// A plain semver `node` dep (no `runtime:` prefix) is not an
/// `engines.runtime` pin — workspaces can depend on the `node`
/// npm package without intending it as the script runner. The
/// helper must skip these.
#[test]
fn ignores_non_runtime_node_dep() {
    let mut deps = HashMap::new();
    deps.insert(
        PkgName::parse("node").expect("parse pkg name"),
        SnapshotDepRef::Plain("22.11.0".parse().expect("parse ver-peer")),
    );
    let snapshot = SnapshotEntry { dependencies: Some(deps), ..SnapshotEntry::default() };
    assert_eq!(find_own_runtime_node_major(&snapshot), None);
}

/// Scoped `node` (`@scope/node`) isn't pnpm's runtime alias —
/// only the bare unscoped `node` alias counts. Matches the
/// sibling [`super::find_runtime_node_major`] check.
#[test]
fn ignores_scoped_node_alias() {
    let mut deps = HashMap::new();
    deps.insert(
        PkgName::parse("@scope/node").expect("parse pkg name"),
        SnapshotDepRef::Plain("runtime:22.11.0".parse().expect("parse ver-peer")),
    );
    let snapshot = SnapshotEntry { dependencies: Some(deps), ..SnapshotEntry::default() };
    assert_eq!(find_own_runtime_node_major(&snapshot), None);
}

/// A snapshot with no `dependencies` map yields `None` — the
/// install-wide fallback handles those.
#[test]
fn empty_dependencies_yields_none() {
    let snapshot = SnapshotEntry::default();
    assert_eq!(find_own_runtime_node_major(&snapshot), None);
}
