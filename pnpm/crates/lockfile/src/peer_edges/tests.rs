use std::{collections::HashMap, fmt::Write, path::Path};

use pretty_assertions::assert_eq;

use super::{PeerEdgeOptions, PeerSatisfactionEdges};
use crate::{Lockfile, PackageKey, PkgName};

const ABC: &str = "abc@1.0.0(peer-a@1.0.0)(peer-c@1.0.0)";

/// `abc` declares `peer-a` as a required peer and `peer-c` as an optional
/// one. `importers` is the YAML of the `importers:` block.
fn abc_lockfile(importers: &str) -> Lockfile {
    let text = format!(
        "lockfileVersion: '9.0'

importers:
{importers}
packages:

  abc@1.0.0:
    resolution: {{integrity: sha512-abc}}
    peerDependencies:
      peer-a: ^1.0.0
      peer-c: ^1.0.0
    peerDependenciesMeta:
      peer-c:
        optional: true

  peer-a@1.0.0:
    resolution: {{integrity: sha512-a}}

  peer-c@1.0.0:
    resolution: {{integrity: sha512-c}}

snapshots:

  {ABC}:
    dependencies:
      peer-a: 1.0.0
    optionalDependencies:
      peer-c: 1.0.0

  peer-a@1.0.0: {{}}

  peer-c@1.0.0: {{}}
",
    );
    parse(&text)
}

fn parse(text: &str) -> Lockfile {
    Lockfile::parse(text, Path::new("pnpm-lock.yaml"))
        .expect("parse lockfile")
        .expect("lockfile is not empty")
}

/// One importer block. `groups` pairs a dependency field with the entries
/// it lists, each an `(alias, version)`.
fn importer(id: &str, groups: &[(&str, &[(&str, &str)])]) -> String {
    let mut text = format!("\n  {id}:\n");
    for (field, entries) in groups {
        writeln!(text, "    {field}:").unwrap();
        for (alias, version) in *entries {
            writeln!(
                text,
                "      {alias}:\n        specifier: {version}\n        version: {version}",
            )
            .unwrap();
        }
    }
    text
}

fn prod_abc_dev_peers(id: &str) -> String {
    importer(
        id,
        &[
            ("dependencies", &[("abc", "1.0.0(peer-a@1.0.0)(peer-c@1.0.0)")]),
            ("devDependencies", &[("peer-a", "1.0.0"), ("peer-c", "1.0.0")]),
        ],
    )
}

fn key(text: &str) -> PackageKey {
    text.parse().expect("parse package key")
}

fn alias(text: &str) -> PkgName {
    text.parse().expect("parse alias")
}

fn classified(lockfile: &Lockfile, options: PeerEdgeOptions) -> Vec<String> {
    let edges = PeerSatisfactionEdges::of_lockfile(lockfile, options);
    let mut found = edges
        .iter()
        .flat_map(|(key, aliases)| {
            aliases
                .iter()
                .map(move |alias| format!("{key} > {alias}"))
        })
        .collect::<Vec<_>>();
    found.sort();
    found
}

const ROOT_RULE: PeerEdgeOptions = PeerEdgeOptions { resolve_peers_from_workspace_root: true };

#[test]
fn only_the_optional_peer_is_a_peer_satisfaction_edge() {
    let lockfile = abc_lockfile(&prod_abc_dev_peers("."));
    assert_eq!(classified(&lockfile, PeerEdgeOptions::default()), [format!("{ABC} > peer-c")]);
}

/// An auto-installed peer is listed by no importer, so the entry is the only
/// thing that provides it.
#[test]
fn a_peer_no_importer_lists_is_followed() {
    let lockfile = abc_lockfile(&importer(
        ".",
        &[("dependencies", &[("abc", "1.0.0(peer-a@1.0.0)(peer-c@1.0.0)")])],
    ));
    assert_eq!(classified(&lockfile, PeerEdgeOptions::default()), Vec::<String>::new());
}

#[test]
fn a_peer_one_reaching_importer_does_not_list_is_followed() {
    let importers = [
        prod_abc_dev_peers("packages/listing"),
        importer(
            "packages/other",
            &[("dependencies", &[("abc", "1.0.0(peer-a@1.0.0)(peer-c@1.0.0)")])],
        ),
    ]
    .concat();
    let lockfile = abc_lockfile(&importers);
    assert_eq!(classified(&lockfile, PeerEdgeOptions::default()), Vec::<String>::new());
}

/// An importer that lists the peer but does not reach the dependent does not
/// decide anything.
#[test]
fn an_importer_that_does_not_reach_the_dependent_is_ignored() {
    let importers = [
        prod_abc_dev_peers("packages/app"),
        importer("packages/other", &[("dependencies", &[("peer-a", "1.0.0")])]),
    ]
    .concat();
    let lockfile = abc_lockfile(&importers);
    assert_eq!(classified(&lockfile, PeerEdgeOptions::default()), [format!("{ABC} > peer-c")]);
}

#[test]
fn a_peer_the_workspace_root_lists_counts_only_with_the_root_rule() {
    let importers = [
        importer(".", &[("devDependencies", &[("peer-c", "1.0.0")])]),
        importer(
            "packages/app",
            &[("dependencies", &[("abc", "1.0.0(peer-a@1.0.0)(peer-c@1.0.0)")])],
        ),
    ]
    .concat();
    let lockfile = abc_lockfile(&importers);
    assert_eq!(classified(&lockfile, PeerEdgeOptions::default()), Vec::<String>::new());
    assert_eq!(classified(&lockfile, ROOT_RULE), [format!("{ABC} > peer-c")]);
}

/// Only a root devDependency counts for every importer: a root production
/// dependency stays installed when the project is installed on its own.
#[test]
fn a_peer_the_workspace_root_lists_as_a_production_dependency_is_followed() {
    let importers = [
        importer(".", &[("dependencies", &[("peer-c", "1.0.0")])]),
        importer(
            "packages/app",
            &[("dependencies", &[("abc", "1.0.0(peer-a@1.0.0)(peer-c@1.0.0)")])],
        ),
    ]
    .concat();
    let lockfile = abc_lockfile(&importers);
    assert_eq!(classified(&lockfile, ROOT_RULE), Vec::<String>::new());
}

/// Classification reads the lockfile's maps by exact key, so aliases named
/// like object members get no special treatment, and a
/// `peerDependenciesMeta` entry alone does not make an alias a peer.
#[test]
fn member_like_aliases_are_classified_by_their_declarations() {
    let lockfile = parse(&format!(
        "lockfileVersion: '9.0'

importers:
{}
packages:

  host@1.0.0:
    resolution: {{integrity: sha512-host}}
    peerDependencies:
      toString: ^1.0.0
    peerDependenciesMeta:
      constructor:
        optional: true
      toString:
        optional: true

  constructor@1.0.0:
    resolution: {{integrity: sha512-constructor}}

  toString@1.0.0:
    resolution: {{integrity: sha512-tostring}}

snapshots:

  host@1.0.0(constructor@1.0.0)(toString@1.0.0):
    optionalDependencies:
      constructor: 1.0.0
      toString: 1.0.0

  constructor@1.0.0: {{}}

  toString@1.0.0: {{}}
",
        importer(
            ".",
            &[
                ("dependencies", &[("host", "1.0.0(constructor@1.0.0)(toString@1.0.0)")]),
                ("devDependencies", &[("constructor", "1.0.0"), ("toString", "1.0.0")]),
            ],
        ),
    ));
    assert_eq!(
        classified(&lockfile, PeerEdgeOptions::default()),
        ["host@1.0.0(constructor@1.0.0)(toString@1.0.0) > toString"],
    );
}

/// Each walk is shared by the targets listed by the same importers. Listings
/// such as `{1, 23}` and `{12, 3}` read the same when their indices are
/// concatenated, so a key built that way would hand one target the other's
/// reach, and both edges below would be followed.
#[test]
fn listings_that_concatenate_alike_do_not_share_a_walk() {
    let mut importers = String::new();
    for index in 0..24 {
        let id = format!("packages/p{index:02}");
        importers += &match index {
            1 | 23 => importer(
                &id,
                &[
                    ("dependencies", &[("one", "1.0.0(t1@1.0.0)")]),
                    ("devDependencies", &[("t1", "1.0.0")]),
                ],
            ),
            3 | 12 => importer(
                &id,
                &[
                    ("dependencies", &[("two", "1.0.0(t2@1.0.0)")]),
                    ("devDependencies", &[("t2", "1.0.0")]),
                ],
            ),
            _ => importer(&id, &[("dependencies", &[("leaf", "1.0.0")])]),
        };
    }
    let lockfile = parse(&format!(
        "lockfileVersion: '9.0'

importers:
{importers}
packages:

  leaf@1.0.0:
    resolution: {{integrity: sha512-leaf}}

  one@1.0.0:
    resolution: {{integrity: sha512-one}}
    peerDependencies:
      t1: ^1.0.0
    peerDependenciesMeta:
      t1:
        optional: true

  two@1.0.0:
    resolution: {{integrity: sha512-two}}
    peerDependencies:
      t2: ^1.0.0
    peerDependenciesMeta:
      t2:
        optional: true

  t1@1.0.0:
    resolution: {{integrity: sha512-t1}}

  t2@1.0.0:
    resolution: {{integrity: sha512-t2}}

snapshots:

  leaf@1.0.0: {{}}

  one@1.0.0(t1@1.0.0):
    dependencies:
      t1: 1.0.0

  two@1.0.0(t2@1.0.0):
    dependencies:
      t2: 1.0.0

  t1@1.0.0: {{}}

  t2@1.0.0: {{}}
",
    ));
    assert_eq!(
        classified(&lockfile, PeerEdgeOptions::default()),
        ["one@1.0.0(t1@1.0.0) > t1", "two@1.0.0(t2@1.0.0) > t2"],
    );
}

#[test]
fn followed_entries_leave_out_the_peer_satisfaction_edges() {
    let lockfile = abc_lockfile(&prod_abc_dev_peers("."));
    let edges = PeerSatisfactionEdges::of_lockfile(&lockfile, PeerEdgeOptions::default());
    let abc = key(ABC);
    let snapshot = &lockfile.snapshots.as_ref().unwrap()[&abc];
    let followed = |include_optional| {
        let mut aliases = edges
            .followed_entries(&abc, snapshot, include_optional)
            .map(|(alias, _)| alias.to_string())
            .collect::<Vec<_>>();
        aliases.sort();
        aliases
    };
    assert_eq!(followed(true), ["peer-a"]);
    assert_eq!(followed(false), ["peer-a"]);
    assert!(edges.contains(&abc, &alias("peer-c")));
    assert!(!edges.contains(&abc, &alias("peer-a")));
}

#[test]
fn prune_dangling_removes_only_edges_to_dropped_targets() {
    let lockfile = abc_lockfile(&prod_abc_dev_peers("."));
    let edges = PeerSatisfactionEdges::of_lockfile(&lockfile, PeerEdgeOptions::default());
    let abc = key(ABC);
    let original = lockfile.snapshots.unwrap();

    let mut kept = original.clone();
    edges.prune_dangling(&mut kept);
    assert_eq!(kept, original);

    let mut pruned: HashMap<_, _> = original
        .iter()
        .filter(|(key, _)| key.to_string() != "peer-c@1.0.0")
        .map(|(key, snapshot)| (key.clone(), snapshot.clone()))
        .collect();
    edges.prune_dangling(&mut pruned);
    let abc_snapshot = &pruned[&abc];
    assert_eq!(abc_snapshot.optional_dependencies, None);
    assert!(
        abc_snapshot.dependencies
            .as_ref()
            .unwrap()
            .contains_key(&alias("peer-a")),
    );
}

#[test]
fn prune_dangling_judges_each_dependency_map_by_its_own_target() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .: {}

packages:

  abc@1.0.0:
    resolution: {integrity: sha512-abc}

snapshots:

  abc@1.0.0:
    dependencies:
      peer-c: 1.0.0
    optionalDependencies:
      peer-c: 2.0.0

  peer-c@2.0.0: {}
",
    );
    let abc = key("abc@1.0.0");
    let edges = std::iter::once((abc.clone(), std::iter::once(alias("peer-c")).collect()))
        .collect::<PeerSatisfactionEdges>();
    let mut snapshots = lockfile.snapshots.unwrap();

    edges.prune_dangling(&mut snapshots);

    let abc_snapshot = &snapshots[&abc];
    assert_eq!(abc_snapshot.dependencies, None);
    assert!(
        abc_snapshot.optional_dependencies
            .as_ref()
            .unwrap()
            .contains_key(&alias("peer-c")),
    );
}

#[test]
fn a_lockfile_without_optional_peers_has_no_peer_satisfaction_edges() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      dep:
        specifier: 1.0.0
        version: 1.0.0

packages:

  dep@1.0.0:
    resolution: {integrity: sha512-dep}

snapshots:

  dep@1.0.0: {}
",
    );
    assert!(PeerSatisfactionEdges::of_lockfile(&lockfile, ROOT_RULE).is_empty());
}
