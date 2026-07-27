use super::find_own_runtime_node_major;
use pacquet_lockfile::{PkgName, SnapshotDepRef, SnapshotEntry};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

/// `dependencies.node: 'runtime:<v>'` is the desugared form the
/// resolver writes when a dep declares its own `engines.runtime`.
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
/// npm package without intending it as the script runner.
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

/// Matches the sibling [`super::find_runtime_node_major`] check:
/// only the bare unscoped `node` alias counts as a runtime pin.
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

/// The install-wide fallback handles snapshots that carry no
/// `dependencies` map of their own.
#[test]
fn empty_dependencies_yields_none() {
    let snapshot = SnapshotEntry::default();
    assert_eq!(find_own_runtime_node_major(&snapshot), None);
}

#[tokio::test]
async fn load_custom_fetcher_picker_is_none_without_a_pnpmfile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let picker = super::load_custom_fetcher_picker(tmp.path())
        .await
        .expect("a missing pnpmfile is not an error");
    assert!(picker.is_none());
}

#[tokio::test]
async fn load_custom_fetcher_picker_is_none_when_pnpmfile_exports_no_fetchers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), "module.exports = { hooks: {} }\n")
        .expect("write pnpmfile");
    let picker = super::load_custom_fetcher_picker(tmp.path())
        .await
        .expect("a fetchers-less pnpmfile is not an error");
    assert!(picker.is_none());
}

#[tokio::test]
async fn load_custom_fetcher_picker_returns_a_picker_for_exported_fetchers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join(".pnpmfile.cjs"),
        "module.exports = { fetchers: [{ canFetch () { return false }, fetch () { return null } }] }\n",
    )
    .expect("write pnpmfile");
    let picker = super::load_custom_fetcher_picker(tmp.path())
        .await
        .expect("a well-formed fetchers export must load")
        .expect("one exported fetcher must yield a picker");
    assert!(!picker.is_empty());
}

/// A pnpmfile that fails to evaluate aborts the install rather than
/// silently proceeding without custom fetchers — a package whose fetch
/// the pnpmfile was meant to intercept must not fall through to the
/// built-in dispatch.
#[tokio::test]
async fn load_custom_fetcher_picker_propagates_a_broken_pnpmfile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), "throw new Error('pnpmfile exploded')\n")
        .expect("write pnpmfile");
    let Err(err) = super::load_custom_fetcher_picker(tmp.path()).await else {
        panic!("a throwing pnpmfile must fail the load");
    };
    assert!(
        matches!(
            &err,
            super::InstallFrozenLockfileError::CustomFetcherHook(hook_err)
                if hook_err.to_string().contains("pnpmfile exploded"),
        ),
        "expected the pnpmfile error to propagate, got {err:?}",
    );
}
