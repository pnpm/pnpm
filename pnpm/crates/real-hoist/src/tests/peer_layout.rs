//! Peer shapes a pnpm lockfile produces that Yarn's model of peers does
//! not cover. Each test hoists a lockfile through [`hoist`], which checks
//! every declared edge under the resolution oracle, and pins the layout
//! that shows which mechanism kept the peer correct.

use super::{
    HoistOpts,
    HoisterResult,
    Lockfile,
    Rc,
    child_of,
    hoist,
};
use std::{
    collections::VecDeque,
    path::Path,
};

fn hoist_lockfile(yaml: &str) -> HoisterResult {
    let lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile is not empty");
    hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed")
}

fn references(node: &HoisterResult) -> Vec<String> {
    node.references
        .borrow()
        .iter()
        .cloned()
        .collect()
}

fn names(node: &HoisterResult) -> Vec<String> {
    let mut names: Vec<String> = node.dependencies
        .borrow()
        .iter()
        .map(|dep| dep.0.name.clone())
        .collect();
    names.sort_unstable();
    names
}

/// The directory that serves `reference`, searched breadth-first.
fn serving(result: &HoisterResult, reference: &str) -> Rc<HoisterResult> {
    let mut pending: VecDeque<Rc<HoisterResult>> = result.dependencies
        .borrow()
        .iter()
        .map(|dep| Rc::clone(&dep.0))
        .collect();
    while let Some(node) = pending.pop_front() {
        if node.references.borrow().contains(reference) {
            return node;
        }
        pending.extend(
            node.dependencies
                .borrow()
                .iter()
                .map(|dep| Rc::clone(&dep.0)),
        );
    }
    panic!("no directory serves {reference}: {result:#?}")
}

/// `m` passes its descendant's `f` peer up to `p`, which provides `f@2`
/// while the root holds `f@1`. A lockfile lists `m`'s transitive peer by
/// name only. Linking it to its provider lets the peer check walk from
/// `c` through `m` to `p`, so `c` stays where `p`'s `f@2` is found, and
/// needs no copy of `f` of its own.
#[test]
fn a_transitive_peer_is_traced_to_its_provider() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      f: {specifier: 1.0.0, version: 1.0.0}
      p: {specifier: 1.0.0, version: 1.0.0}
packages:
  c@1.0.0: {resolution: {integrity: sha512-c}, peerDependencies: {f: '*'}}
  f@1.0.0: {resolution: {integrity: sha512-f1}}
  f@2.0.0: {resolution: {integrity: sha512-f2}}
  m@1.0.0: {resolution: {integrity: sha512-m}}
  p@1.0.0: {resolution: {integrity: sha512-p}}
snapshots:
  c@1.0.0(f@2.0.0):
    dependencies: {f: 2.0.0}
  f@1.0.0: {}
  f@2.0.0: {}
  m@1.0.0(f@2.0.0):
    dependencies: {c: 1.0.0(f@2.0.0)}
    transitivePeerDependencies: [f]
  p@1.0.0:
    dependencies: {f: 2.0.0, m: 1.0.0(f@2.0.0)}
",
    );
    assert_eq!(names(&result), ["f", "p"], "c must not reach the root's f@1: {result:#?}");
    let c = serving(&result, "c@1.0.0(f@2.0.0)");
    assert!(names(&c).is_empty(), "c finds p's f@2 above it: {c:#?}");
}

/// `b@1` provides `f@2` to `e`'s peer, then a nested pass hoists that
/// `f@2` from `b` up into `a`. `b` no longer holds `f`, but the provider
/// did not go away: it moved up. The peer check keeps walking to `a`, so
/// the next round does not deduplicate `e` into the root copy, which is
/// bound to `f@1`.
#[test]
fn a_peer_provider_hoisted_away_still_binds_the_peer() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      a: {specifier: 1.0.0, version: 1.0.0}
      b: {specifier: 2.0.0, version: 2.0.0}
      e: {specifier: 1.0.0, version: 1.0.0(f@1.0.0)}
      f: {specifier: 1.0.0, version: 1.0.0}
packages:
  a@1.0.0: {resolution: {integrity: sha512-a}}
  b@1.0.0: {resolution: {integrity: sha512-b1}}
  b@2.0.0: {resolution: {integrity: sha512-b2}}
  e@1.0.0: {resolution: {integrity: sha512-e}, peerDependencies: {f: '*'}}
  f@1.0.0: {resolution: {integrity: sha512-f1}}
  f@2.0.0: {resolution: {integrity: sha512-f2}}
snapshots:
  a@1.0.0:
    dependencies: {b: 1.0.0}
  b@1.0.0:
    dependencies: {e: 1.0.0(f@2.0.0), f: 2.0.0}
  b@2.0.0: {}
  e@1.0.0(f@1.0.0):
    dependencies: {f: 1.0.0}
  e@1.0.0(f@2.0.0):
    dependencies: {f: 2.0.0}
  f@1.0.0: {}
  f@2.0.0: {}
",
    );
    assert_eq!(references(&child_of(&result, "e")), ["e@1.0.0(f@1.0.0)"]);
    let nested = serving(&result, "e@1.0.0(f@2.0.0)");
    assert!(names(&nested).is_empty(), "e finds the hoisted f@2 above it: {nested:#?}");
}

/// `core@1` depends on `helper`, whose `core` peer is `core@1` itself, the
/// shape of `@babel/core` and its helpers. From inside `core`, a `require`
/// of `core` finds `core` through its parent's `node_modules`, so the
/// helper may move up next to the nested `core@1`, but not to the root,
/// where `core` is `core@2`.
#[test]
fn a_package_that_is_its_dependency_s_peer_keeps_it_nested() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      core: {specifier: 2.0.0, version: 2.0.0}
      x: {specifier: 1.0.0, version: 1.0.0}
packages:
  core@1.0.0: {resolution: {integrity: sha512-core1}}
  core@2.0.0: {resolution: {integrity: sha512-core2}}
  helper@1.0.0: {resolution: {integrity: sha512-helper}, peerDependencies: {core: '*'}}
  x@1.0.0: {resolution: {integrity: sha512-x}}
snapshots:
  core@1.0.0:
    dependencies: {helper: 1.0.0(core@1.0.0)}
  core@2.0.0:
    dependencies: {helper: 1.0.0(core@2.0.0)}
  helper@1.0.0(core@1.0.0):
    dependencies: {core: 1.0.0}
  helper@1.0.0(core@2.0.0):
    dependencies: {core: 2.0.0}
  x@1.0.0:
    dependencies: {core: 1.0.0}
",
    );
    assert_eq!(references(&child_of(&result, "helper")), ["helper@1.0.0(core@2.0.0)"]);
    let x = child_of(&result, "x");
    assert_eq!(names(&x), ["core", "helper"], "the helper sits next to core@1: {x:#?}");
    assert!(names(&child_of(&x, "helper")).is_empty(), "and finds it without a copy of its own");
}

/// The project installs `webpack@5` as `webpack-5`, and pnpm binds
/// `plugin`'s `webpack` peer to it. Walking up by the name `webpack` only
/// finds the root's `webpack@4`, so the plugin holds `webpack@5` as a
/// dependency of its own.
#[test]
fn a_peer_bound_to_an_aliased_dependency_gets_its_own_copy() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      plugin: {specifier: 1.0.0, version: 1.0.0(webpack@5.0.0)}
      webpack: {specifier: 4.0.0, version: 4.0.0}
      webpack-5: {specifier: npm:webpack@5.0.0, version: webpack@5.0.0}
packages:
  plugin@1.0.0: {resolution: {integrity: sha512-plugin}, peerDependencies: {webpack: '*'}}
  webpack@4.0.0: {resolution: {integrity: sha512-webpack4}}
  webpack@5.0.0: {resolution: {integrity: sha512-webpack5}}
snapshots:
  plugin@1.0.0(webpack@5.0.0):
    dependencies: {webpack: 5.0.0}
  webpack@4.0.0: {}
  webpack@5.0.0: {}
",
    );
    assert_eq!(references(&child_of(&result, "webpack")), ["webpack@4.0.0"]);
    let plugin = child_of(&result, "plugin");
    assert_eq!(references(&child_of(&plugin, "webpack")), ["webpack@5.0.0"]);
}

/// `webpack@5`, installed as `webpack-5`, depends on a plugin whose
/// `webpack` peer is `webpack@5` itself. Hoisting the plugin's own copy of
/// `webpack` into `webpack-5` must not drop it as served by the root: the
/// root is not found under the name `webpack`.
#[test]
fn a_root_under_an_alias_does_not_absorb_its_own_package() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      webpack: {specifier: 4.0.0, version: 4.0.0}
      webpack-5: {specifier: npm:webpack@5.0.0, version: webpack@5.0.0}
packages:
  minimizer@1.0.0: {resolution: {integrity: sha512-minimizer}, peerDependencies: {webpack: '*'}}
  webpack@4.0.0: {resolution: {integrity: sha512-webpack4}}
  webpack@5.0.0: {resolution: {integrity: sha512-webpack5}}
snapshots:
  minimizer@1.0.0(webpack@5.0.0):
    dependencies: {webpack: 5.0.0}
  webpack@4.0.0: {}
  webpack@5.0.0:
    dependencies: {minimizer: 1.0.0(webpack@5.0.0)}
",
    );
    let minimizer = serving(&result, "minimizer@1.0.0(webpack@5.0.0)");
    assert_eq!(references(&child_of(&minimizer, "webpack")), ["webpack@5.0.0"]);
}
