use super::groups;
use crate::{GroupSelection, SkippedSnapshots};
use pnpm_lockfile::{Lockfile, PackageKey, PeerEdgeOptions};
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;
use std::{collections::HashSet, path::Path};

const ABC: &str = "abc@1.0.0(peer-a@1.0.0)(peer-c@1.0.0)";

/// `abc` has a required peer `peer-a` and an optional peer `peer-c`, both
/// provided by the root importer's `devDependencies`.
const LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      abc:
        specifier: 1.0.0
        version: 1.0.0(peer-a@1.0.0)(peer-c@1.0.0)
    devDependencies:
      peer-a:
        specifier: 1.0.0
        version: 1.0.0
      peer-c:
        specifier: 1.0.0
        version: 1.0.0

packages:

  abc@1.0.0:
    resolution: {integrity: sha512-abc}
    peerDependencies:
      peer-a: ^1.0.0
      peer-c: ^1.0.0
    peerDependenciesMeta:
      peer-c:
        optional: true

  peer-a@1.0.0:
    resolution: {integrity: sha512-a}

  peer-c@1.0.0:
    resolution: {integrity: sha512-c}

snapshots:

  abc@1.0.0(peer-a@1.0.0)(peer-c@1.0.0):
    dependencies:
      peer-a: 1.0.0
    optionalDependencies:
      peer-c: 1.0.0

  peer-a@1.0.0: {}

  peer-c@1.0.0: {}
";

fn lockfile() -> Lockfile {
    Lockfile::parse(LOCKFILE, Path::new("pnpm-lock.yaml"))
        .expect("parse lockfile")
        .expect("lockfile is not empty")
}

fn prod() -> GroupSelection {
    groups(IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: true,
    })
}

fn snapshot_keys(lockfile: &Lockfile) -> Vec<String> {
    let mut keys = lockfile.snapshots
        .iter()
        .flat_map(|snapshots| snapshots.keys())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn abc_aliases(lockfile: &Lockfile) -> Vec<String> {
    let abc = &lockfile.snapshots.as_ref().unwrap()[&ABC.parse::<PackageKey>().unwrap()];
    let mut aliases = [abc.dependencies.as_ref(), abc.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|entries| entries.keys())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    aliases.sort();
    aliases
}

#[test]
fn a_prod_closure_drops_an_optional_peer_only_a_dev_dependency_provides() {
    let closure = super::super::materialization_closure(
        &lockfile(),
        Path::new(""),
        &HashSet::from([Lockfile::ROOT_IMPORTER_KEY.to_string()]),
        prod(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(snapshot_keys(&closure.lockfile), [ABC, "peer-a@1.0.0"]);
    assert_eq!(abc_aliases(&closure.lockfile), ["peer-a"]);
    let packages = closure.lockfile.packages.as_ref().unwrap();
    assert!(!packages.contains_key(&"peer-c@1.0.0".parse::<PackageKey>().unwrap()));
}

#[test]
fn the_root_rule_decides_whether_a_root_dev_dependency_provides_the_peer() {
    let text = LOCKFILE.replace(
        "  .:\n    dependencies:\n      abc:\n        specifier: 1.0.0\n        version: 1.0.0(peer-a@1.0.0)(peer-c@1.0.0)\n    devDependencies:",
        "  packages/app:\n    dependencies:\n      abc:\n        specifier: 1.0.0\n        version: 1.0.0(peer-a@1.0.0)(peer-c@1.0.0)\n      peer-a:\n        specifier: 1.0.0\n        version: 1.0.0\n\n  .:\n    devDependencies:",
    );
    let lockfile = Lockfile::parse(&text, Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    assert!(lockfile.importers["packages/app"].dev_dependencies.is_none());
    let app = HashSet::from(["packages/app".to_string()]);
    let closure = |resolve_peers_from_workspace_root| {
        super::super::materialization_closure(
            &lockfile,
            Path::new(""),
            &app,
            GroupSelection {
                peer_edges: PeerEdgeOptions { resolve_peers_from_workspace_root },
                ..prod()
            },
            &SkippedSnapshots::new(),
        )
        .lockfile
    };

    assert_eq!(snapshot_keys(&closure(false)), [ABC, "peer-a@1.0.0", "peer-c@1.0.0"]);
    assert_eq!(snapshot_keys(&closure(true)), [ABC, "peer-a@1.0.0"]);
}

#[test]
fn the_current_lockfile_of_a_prod_install_does_not_record_the_dropped_peer_edge() {
    let current =
        super::super::filter_lockfile_for_current(&lockfile(), prod(), &SkippedSnapshots::new());

    assert_eq!(snapshot_keys(&current), [ABC, "peer-a@1.0.0"]);
    assert_eq!(abc_aliases(&current), ["peer-a"]);
    assert_eq!(
        super::super::filter_lockfile_for_current(&current, prod(), &SkippedSnapshots::new()),
        current,
        "filtering the current lockfile again must change nothing",
    );
}

#[test]
fn a_closure_over_every_group_keeps_every_peer_edge() {
    let lockfile = lockfile();
    let closure = super::super::materialization_closure(
        &lockfile,
        Path::new(""),
        &HashSet::from([Lockfile::ROOT_IMPORTER_KEY.to_string()]),
        GroupSelection::all(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.lockfile.snapshots, lockfile.snapshots);
}

/// Every entry a prod filter removes from a retained snapshot points at a
/// snapshot the filter drops, and every entry it keeps points at one it keeps.
#[test]
fn a_big_lockfile_loses_no_edge_whose_target_the_filter_keeps() {
    let lockfile =
        Lockfile::parse(pnpm_testing_utils::fixtures::BIG_LOCKFILE, Path::new("big.yaml"))
            .expect("parse the big lockfile")
            .expect("the big lockfile is not empty");
    let without_dev = super::super::filter_lockfile_for_current(
        &lockfile,
        GroupSelection {
            included: IncludedDependencies {
                dependencies: true,
                dev_dependencies: false,
                optional_dependencies: true,
            },
            peer_edges: PeerEdgeOptions { resolve_peers_from_workspace_root: true },
        },
        &SkippedSnapshots::new(),
    );
    let source = lockfile.snapshots.as_ref().unwrap();
    let retained = without_dev.snapshots.as_ref().unwrap();

    for (key, snapshot) in retained {
        let entries = [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()];
        let original = &source[key];
        for (alias, dep_ref) in
            [original.dependencies.as_ref(), original.optional_dependencies.as_ref()]
                .into_iter()
                .flatten()
                .flatten()
        {
            let Some(target) = dep_ref.resolve(alias) else { continue };
            let kept = entries
                .iter()
                .flatten()
                .any(|entries| entries.contains_key(alias));
            assert_eq!(kept, retained.contains_key(&target), "{key} > {alias}");
        }
    }
}
