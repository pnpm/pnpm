use pipe_trait::Pipe;
use pnpm_lockfile::{Lockfile, PkgNameVerPeer};
use std::path::Path;

use super::TargetDistances;
use crate::{UpdateReuseScope, UpdateTargets, VersionLine};

/// `a` depends on `b`, which depends on `c`. `d` depends on `c` through
/// a `link:`, which the snapshot graph does not follow.
const CHAIN: &str = "lockfileVersion: '9.0'
packages:
  a@1.0.0:
    resolution: {integrity: sha512-a}
  b@1.0.0:
    resolution: {integrity: sha512-b}
  c@1.0.0:
    resolution: {integrity: sha512-c}
  d@1.0.0:
    resolution: {integrity: sha512-d}
snapshots:
  a@1.0.0:
    dependencies:
      b: 1.0.0
  b@1.0.0:
    dependencies:
      c: 1.0.0
  c@1.0.0: {}
  d@1.0.0:
    dependencies:
      c: link:../c
";

/// `a` and `b` depend on each other, and `a` also depends on `c`.
const CYCLE: &str = "lockfileVersion: '9.0'
packages:
  a@1.0.0:
    resolution: {integrity: sha512-a}
  b@1.0.0:
    resolution: {integrity: sha512-b}
  c@1.0.0:
    resolution: {integrity: sha512-c}
snapshots:
  a@1.0.0:
    dependencies:
      b: 1.0.0
      c: 1.0.0
  b@1.0.0:
    dependencies:
      a: 1.0.0
  c@1.0.0: {}
";

fn parse(lockfile: &str) -> Lockfile {
    Lockfile::parse(lockfile, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile has content")
}

fn key(key: &str) -> PkgNameVerPeer {
    key.parse().expect("parse the snapshot key")
}

fn update_of(targets: &[(&str, Option<VersionLine>)]) -> UpdateReuseScope {
    targets
        .iter()
        .map(|(name, line)| ((*name).to_string(), *line))
        .collect::<UpdateTargets>()
        .pipe(UpdateReuseScope::Except)
}

fn distances_of(lockfile: &str, targets: &[&str]) -> TargetDistances {
    let targets = targets
        .iter()
        .map(|name| (*name, None))
        .collect::<Vec<_>>();
    TargetDistances::compute(Some(&parse(lockfile)), &update_of(&targets))
}

#[test]
fn each_node_is_as_far_as_its_shortest_path_to_a_target() {
    let distances = distances_of(CHAIN, &["c"]);
    assert_eq!(distances.to_target(&key("c@1.0.0")), Some(0));
    assert_eq!(distances.to_target(&key("b@1.0.0")), Some(1));
    assert_eq!(distances.to_target(&key("a@1.0.0")), Some(2));
}

#[test]
fn a_link_edge_reaches_no_target() {
    let distances = distances_of(CHAIN, &["c"]);
    assert_eq!(distances.to_target(&key("d@1.0.0")), None);
}

#[test]
fn a_cycle_is_measured_the_short_way_round() {
    let distances = distances_of(CYCLE, &["c"]);
    assert_eq!(distances.to_target(&key("c@1.0.0")), Some(0));
    assert_eq!(distances.to_target(&key("a@1.0.0")), Some(1));
    assert_eq!(distances.to_target(&key("b@1.0.0")), Some(2));
}

#[test]
fn a_target_at_a_version_line_the_node_is_not_on_is_not_reached() {
    let line = VersionLine::of(&"2.0.0".parse().expect("parse the version"));
    let scope = update_of(&[("c", Some(line))]);
    let distances = TargetDistances::compute(Some(&parse(CHAIN)), &scope);
    assert_eq!(distances.to_target(&key("c@1.0.0")), None);
    assert_eq!(distances.to_target(&key("a@1.0.0")), None);
}

#[test]
fn an_update_of_everything_finds_a_target_at_every_node() {
    let distances = TargetDistances::compute(Some(&parse(CHAIN)), &UpdateReuseScope::None);
    assert_eq!(distances.to_target(&key("a@1.0.0")), Some(0));
    assert_eq!(distances.to_target(&key("not-in-the-lockfile@1.0.0")), Some(0));
}

#[test]
fn an_install_has_no_targets() {
    let distances = TargetDistances::compute(Some(&parse(CHAIN)), &UpdateReuseScope::All);
    assert_eq!(distances.to_target(&key("c@1.0.0")), None);
}

#[test]
fn without_a_prior_lockfile_nothing_reaches_a_target() {
    let distances = TargetDistances::compute(None, &update_of(&[("c", None)]));
    assert_eq!(distances.to_target(&key("c@1.0.0")), None);
}
