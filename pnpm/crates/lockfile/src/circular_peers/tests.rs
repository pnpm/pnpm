use std::{fmt::Write, path::Path};

use super::has_circular_peers;
use crate::Lockfile;

fn parse(text: &str) -> Lockfile {
    Lockfile::parse(text, Path::new("pnpm-lock.yaml"))
        .expect("parse lockfile")
        .expect("lockfile is not empty")
}

fn has_cycle(lockfile: &Lockfile) -> bool {
    has_circular_peers(
        lockfile.snapshots.as_ref().expect("snapshots"),
        lockfile.packages.as_ref().expect("packages"),
    )
}

fn lockfile(packages: &str, snapshots: &str) -> Lockfile {
    parse(&format!(
        "lockfileVersion: '9.0'

importers:

  .: {{}}

packages:
{packages}

snapshots:
{snapshots}
",
    ))
}

#[test]
fn a_peer_cycle_leads_back_to_its_start() {
    let lockfile = lockfile(
        "
  a@1.0.0:
    resolution: {integrity: sha512-a}
    peerDependencies:
      b: ^1.0.0

  b@1.0.0:
    resolution: {integrity: sha512-b}
    peerDependencies:
      a: ^1.0.0
    peerDependenciesMeta:
      a:
        optional: true
",
        "
  a@1.0.0:
    dependencies:
      b: 1.0.0

  b@1.0.0:
    optionalDependencies:
      a: 1.0.0
",
    );
    assert!(has_cycle(&lockfile));
}

#[test]
fn a_cycle_of_ordinary_dependencies_is_not_a_peer_cycle() {
    let lockfile = lockfile(
        "
  a@1.0.0:
    resolution: {integrity: sha512-a}

  b@1.0.0:
    resolution: {integrity: sha512-b}
",
        "
  a@1.0.0:
    dependencies:
      b: 1.0.0

  b@1.0.0:
    dependencies:
      a: 1.0.0
",
    );
    assert!(!has_cycle(&lockfile));
}

/// An entry's alias names the target rather than the dependent, so a chain
/// of differently named packages is not read as every package peering on
/// itself.
#[test]
fn a_peer_chain_that_ends_in_a_leaf_is_not_a_cycle() {
    let lockfile = lockfile(
        "
  a@1.0.0:
    resolution: {integrity: sha512-a}
    peerDependencies:
      b: ^1.0.0

  b@1.0.0:
    resolution: {integrity: sha512-b}
    peerDependencies:
      c: ^1.0.0

  c@1.0.0:
    resolution: {integrity: sha512-c}
",
        "
  a@1.0.0:
    dependencies:
      b: 1.0.0

  b@1.0.0:
    dependencies:
      c: 1.0.0

  c@1.0.0: {}
",
    );
    assert!(!has_cycle(&lockfile));
}

#[test]
fn a_cycle_is_found_beside_a_chain_that_has_none() {
    let lockfile = lockfile(
        "
  chain-a@1.0.0:
    resolution: {integrity: sha512-chain-a}
    peerDependencies:
      chain-b: ^1.0.0

  chain-b@1.0.0:
    resolution: {integrity: sha512-chain-b}

  loop-a@1.0.0:
    resolution: {integrity: sha512-loop-a}
    peerDependencies:
      loop-b: ^1.0.0

  loop-b@1.0.0:
    resolution: {integrity: sha512-loop-b}
    peerDependencies:
      loop-a: ^1.0.0
",
        "
  chain-a@1.0.0:
    dependencies:
      chain-b: 1.0.0

  chain-b@1.0.0: {}

  loop-a@1.0.0:
    dependencies:
      loop-b: 1.0.0

  loop-b@1.0.0:
    dependencies:
      loop-a: 1.0.0
",
    );
    assert!(has_cycle(&lockfile));
}

/// A snapshot key names the peers its package was installed with, while the
/// declarations behind that key sit under the bare one.
#[test]
fn a_peer_suffixed_snapshot_is_declared_by_its_bare_key() {
    let lockfile = lockfile(
        "
  a@1.0.0:
    resolution: {integrity: sha512-a}
    peerDependencies:
      b: ^1.0.0

  b@1.0.0:
    resolution: {integrity: sha512-b}
    peerDependencies:
      a: ^1.0.0
",
        "
  a@1.0.0(b@1.0.0):
    dependencies:
      b: 1.0.0(a@1.0.0)

  b@1.0.0(a@1.0.0):
    dependencies:
      a: 1.0.0(b@1.0.0)
",
    );
    assert!(has_cycle(&lockfile));
}

#[test]
fn a_peer_entry_the_lockfile_has_no_snapshot_for_ends_the_chain() {
    let lockfile = lockfile(
        "
  a@1.0.0:
    resolution: {integrity: sha512-a}
    peerDependencies:
      b: ^1.0.0

  b@1.0.0:
    resolution: {integrity: sha512-b}
    peerDependencies:
      a: ^1.0.0
",
        "
  a@1.0.0:
    dependencies:
      b: 2.0.0

  b@1.0.0:
    dependencies:
      a: 1.0.0
",
    );
    assert!(!has_cycle(&lockfile));
}

#[test]
fn a_link_entry_binds_no_peer() {
    let lockfile = lockfile(
        "
  a@1.0.0:
    resolution: {integrity: sha512-a}
    peerDependencies:
      b: ^1.0.0

  b@1.0.0:
    resolution: {integrity: sha512-b}
",
        "
  a@1.0.0:
    dependencies:
      b: link:packages/b

  b@1.0.0: {}
",
    );
    assert!(!has_cycle(&lockfile));
}

/// A chain of this length leaves a recursive walk no stack to run on.
#[test]
fn a_peer_chain_deeper_than_the_stack_does_not_overflow_it() {
    const LENGTH: usize = 50_000;
    let mut packages = String::new();
    let mut snapshots = String::new();
    for index in 0..LENGTH {
        writeln!(packages, "\n  p{index}@1.0.0:\n    resolution: {{integrity: sha512-p}}").unwrap();
        if index + 1 < LENGTH {
            write!(packages, "    peerDependencies:\n      p{}: ^1.0.0\n", index + 1).unwrap();
            writeln!(
                snapshots,
                "\n  p{index}@1.0.0:\n    dependencies:\n      p{}: 1.0.0",
                index + 1,
            )
            .unwrap();
        } else {
            writeln!(snapshots, "\n  p{index}@1.0.0: {{}}").unwrap();
        }
    }
    let lockfile = lockfile(&packages, &snapshots);
    assert!(!has_cycle(&lockfile));
}
