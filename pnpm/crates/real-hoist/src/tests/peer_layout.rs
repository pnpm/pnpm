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
use crate::{
    RegularPeers,
    build_tree,
    engine,
    repair,
    tree,
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

/// How many peers the first hoist of the lockfile resolves to the wrong
/// package, before any repair.
fn first_pass_misses(yaml: &str) -> usize {
    let lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile is not empty");
    let opts = HoistOpts::default();
    let (tree, bound_peers) =
        build_tree(&lockfile, &opts, &RegularPeers::new()).expect("build the tree");
    let result = engine::hoist(&tree, &opts);
    repair::misresolved_peers(&result, &bound_peers).len()
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
    let dependent = serving(&result, "c@1.0.0(f@2.0.0)");
    assert!(names(&dependent).is_empty(), "c finds p's f@2 above it: {dependent:#?}");
}

/// With `dedupePeers`, `m`'s key names its transitive peer's provider as
/// `f@2.0.0`, without the provider's own peer, and the snapshot is keyed
/// `f@2.0.0(r@1.0.0)`. `c`, which declares the peer, records the full key,
/// and `m` is linked to that provider.
#[test]
fn a_transitive_peer_is_traced_to_a_provider_with_peers_of_its_own() {
    let yaml = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      f: {specifier: 1.0.0, version: 1.0.0}
      p: {specifier: 1.0.0, version: 1.0.0}
packages:
  c@1.0.0: {resolution: {integrity: sha512-c}, peerDependencies: {f: '*'}}
  f@1.0.0: {resolution: {integrity: sha512-f1}}
  f@2.0.0: {resolution: {integrity: sha512-f2}, peerDependencies: {r: '*'}}
  m@1.0.0: {resolution: {integrity: sha512-m}}
  p@1.0.0: {resolution: {integrity: sha512-p}}
  r@1.0.0: {resolution: {integrity: sha512-r}}
snapshots:
  c@1.0.0(f@2.0.0):
    dependencies: {f: 2.0.0(r@1.0.0)}
  f@1.0.0: {}
  f@2.0.0(r@1.0.0):
    dependencies: {r: 1.0.0}
  m@1.0.0(f@2.0.0):
    dependencies: {c: 1.0.0(f@2.0.0)}
    transitivePeerDependencies: [f]
  p@1.0.0:
    dependencies: {f: 2.0.0(r@1.0.0), m: 1.0.0(f@2.0.0), r: 1.0.0}
  r@1.0.0: {}
";
    assert_eq!(first_pass_misses(yaml), 0, "the first pass needs no repair");
    let result = hoist_lockfile(yaml);
    assert_eq!(names(&result), ["f", "p", "r"], "c must not reach the root's f@1: {result:#?}");
    let dependent = serving(&result, "c@1.0.0(f@2.0.0)");
    assert!(names(&dependent).is_empty(), "c finds p's f@2 above it: {dependent:#?}");
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
    let holder = child_of(&result, "x");
    assert_eq!(names(&holder), ["core", "helper"], "the helper sits next to core@1: {holder:#?}");
    assert!(
        names(&child_of(&holder, "helper")).is_empty(),
        "and finds it without a copy of its own",
    );
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

/// The project installs `webpack@5` only as `webpack-5`, and pnpm binds
/// `plugin`'s `webpack` peer to it. The project gets a second copy under
/// the peer's name, where the plugin finds it, and the first pass already
/// resolves every peer.
#[test]
fn a_peer_bound_to_a_sibling_under_an_alias_finds_it_under_its_own_name() {
    let yaml = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      plugin: {specifier: 1.0.0, version: 1.0.0(webpack@5.0.0)}
      webpack-5: {specifier: npm:webpack@5.0.0, version: webpack@5.0.0}
packages:
  plugin@1.0.0: {resolution: {integrity: sha512-plugin}, peerDependencies: {webpack: '*'}}
  webpack@5.0.0: {resolution: {integrity: sha512-webpack5}}
snapshots:
  plugin@1.0.0(webpack@5.0.0):
    dependencies: {webpack: 5.0.0}
  webpack@5.0.0: {}
";
    assert_eq!(first_pass_misses(yaml), 0, "the first pass needs no repair");
    let result = hoist_lockfile(yaml);
    assert_eq!(names(&result), ["plugin", "webpack", "webpack-5"]);
    assert_eq!(references(&child_of(&result, "webpack")), ["webpack@5.0.0"]);
    assert!(
        names(&child_of(&result, "plugin")).is_empty(),
        "the plugin holds no copy: {result:#?}",
    );
}

/// `webpack@5`, installed as `webpack-5`, depends on a plugin whose
/// `webpack` peer is `webpack@5` itself. From inside `webpack-5` the name
/// `webpack` leads to the root's `webpack@4`, so `webpack-5` holds a copy
/// of itself under that name.
#[test]
fn an_aliased_package_that_is_its_dependency_s_peer_holds_itself_by_name() {
    let yaml = "lockfileVersion: '9.0'
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
";
    assert_eq!(first_pass_misses(yaml), 0, "the first pass needs no repair");
    let result = hoist_lockfile(yaml);
    let aliased = child_of(&result, "webpack-5");
    assert_eq!(names(&aliased), ["minimizer", "webpack"], "{result:#?}");
    assert_eq!(references(&child_of(&aliased, "webpack")), ["webpack@5.0.0"]);
    assert!(names(&child_of(&aliased, "minimizer")).is_empty(), "the plugin holds no copy");
}

/// `minimizer`'s `webpack` peer is `webpack@5`, installed as `webpack-5`,
/// and `wrapper`, between them, does not record the peer, so the repair
/// gives the plugin a copy of its own. The root holds other versions of
/// `wrapper` and `minimizer`, which keeps both inside `webpack-5`. Hoisting
/// the plugin's copy into `webpack-5` must not drop it as served by that
/// root: the root is not found under the name `webpack`.
#[test]
fn a_root_under_an_alias_does_not_absorb_its_own_package() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      minimizer: {specifier: 2.0.0, version: 2.0.0}
      webpack: {specifier: 4.0.0, version: 4.0.0}
      webpack-5: {specifier: npm:webpack@5.0.0, version: webpack@5.0.0}
      wrapper: {specifier: 2.0.0, version: 2.0.0}
packages:
  minimizer@1.0.0: {resolution: {integrity: sha512-minimizer1}, peerDependencies: {webpack: '*'}}
  minimizer@2.0.0: {resolution: {integrity: sha512-minimizer2}}
  webpack@4.0.0: {resolution: {integrity: sha512-webpack4}}
  webpack@5.0.0: {resolution: {integrity: sha512-webpack5}}
  wrapper@1.0.0: {resolution: {integrity: sha512-wrapper1}}
  wrapper@2.0.0: {resolution: {integrity: sha512-wrapper2}}
snapshots:
  minimizer@1.0.0(webpack@5.0.0):
    dependencies: {webpack: 5.0.0}
  minimizer@2.0.0: {}
  webpack@4.0.0: {}
  webpack@5.0.0:
    dependencies: {wrapper: 1.0.0}
  wrapper@1.0.0:
    dependencies: {minimizer: 1.0.0(webpack@5.0.0)}
  wrapper@2.0.0: {}
",
    );
    let aliased = child_of(&result, "webpack-5");
    let minimizer = serving(&aliased, "minimizer@1.0.0(webpack@5.0.0)");
    assert_eq!(references(&child_of(&minimizer, "webpack")), ["webpack@5.0.0"], "{result:#?}");
}

/// The key `peer` of the snapshot `key` is bound to.
fn provider(yaml: &str, key: &str, peer: &str) -> Option<String> {
    let lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile is not empty");
    let key = key.parse().expect("parse the key");
    let snapshot = &lockfile.snapshots.as_ref().expect("snapshots")[&key];
    tree::peers::provider_of(&key, snapshot, peer, &lockfile).map(|provider| provider.to_string())
}

/// `m`'s key names its transitive peer's provider as `f@1.0.0`. `a` holds
/// `f@1.0.0(r@1.0.0)` as a dependency of its own, and `c` takes `f` as a
/// peer bound to `f@1.0.0(r@2.0.0)`. Only `c` says what the peer is bound
/// to, though `a`'s key sorts first.
#[test]
fn a_dependency_of_its_own_is_not_taken_for_a_peer_binding() {
    let yaml = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      m: {specifier: 1.0.0, version: 1.0.0(f@1.0.0)}
packages:
  a@1.0.0: {resolution: {integrity: sha512-a}}
  c@1.0.0: {resolution: {integrity: sha512-c}, peerDependencies: {f: '*'}}
  f@1.0.0: {resolution: {integrity: sha512-f}, peerDependencies: {r: '*'}}
  m@1.0.0: {resolution: {integrity: sha512-m}}
  r@1.0.0: {resolution: {integrity: sha512-r1}}
  r@2.0.0: {resolution: {integrity: sha512-r2}}
snapshots:
  a@1.0.0:
    dependencies: {f: 1.0.0(r@1.0.0), r: 1.0.0}
  c@1.0.0(f@1.0.0):
    dependencies: {f: 1.0.0(r@2.0.0)}
  f@1.0.0(r@1.0.0):
    dependencies: {r: 1.0.0}
  f@1.0.0(r@2.0.0):
    dependencies: {r: 2.0.0}
  m@1.0.0(f@1.0.0):
    dependencies: {a: 1.0.0, c: 1.0.0(f@1.0.0)}
    transitivePeerDependencies: [f]
  r@1.0.0: {}
  r@2.0.0: {}
";
    assert_eq!(provider(yaml, "m@1.0.0(f@1.0.0)", "f").as_deref(), Some("f@1.0.0(r@2.0.0)"));
}

/// The tree is built from the optional entry of an alias listed in both
/// dependency groups, so that entry is also the one a peer is read from.
#[test]
fn a_peer_listed_in_both_dependency_groups_is_read_from_the_optional_one() {
    let yaml = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      k: {specifier: 1.0.0, version: 1.0.0(f@2.0.0)}
packages:
  f@1.0.0: {resolution: {integrity: sha512-f1}}
  f@2.0.0: {resolution: {integrity: sha512-f2}}
  k@1.0.0: {resolution: {integrity: sha512-k}, peerDependencies: {f: '*'}}
snapshots:
  f@1.0.0: {}
  f@2.0.0: {}
  k@1.0.0(f@2.0.0):
    dependencies: {f: 1.0.0}
    optionalDependencies: {f: 2.0.0}
";
    assert_eq!(provider(yaml, "k@1.0.0(f@2.0.0)", "f").as_deref(), Some("f@2.0.0"));
}

/// `package10` takes `provider2` from its parent `package02` and depends on
/// `package13@2`. `packages/project2` uses `package13@1`, so `package13@2`
/// goes no higher than `package02`. When `provider2` later moves up to the
/// project, `package10` may not follow it: from the project it would find
/// `package13@1`. [`hoist`] checks that. The lockfile is a generated one,
/// reduced to what the layout needs.
#[test]
fn a_package_waiting_on_its_peer_provider_stays_where_its_dependency_resolves() {
    let result = hoist_lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      package27: {specifier: '*', version: 1.0.0}
  packages/project1:
    dependencies:
      package02: {specifier: '*', version: 2.0.0}
      package26: {specifier: '*', version: 1.0.0}
  packages/project2:
    dependencies:
      package02: {specifier: '*', version: 1.0.0}
      package13: {specifier: '*', version: 1.0.0}
      package20: {specifier: '*', version: 2.0.0(provider0@2.0.0)}
      provider0: {specifier: '*', version: 2.0.0}
packages:
  package02@1.0.0: {resolution: {integrity: sha512-a}}
  package02@2.0.0: {resolution: {integrity: sha512-b}}
  package10@1.0.0: {resolution: {integrity: sha512-c}, peerDependencies: {provider2: '*'}}
  package13@1.0.0: {resolution: {integrity: sha512-d}}
  package13@2.0.0: {resolution: {integrity: sha512-e}}
  package14@1.0.0: {resolution: {integrity: sha512-f}}
  package20@1.0.0: {resolution: {integrity: sha512-g}, peerDependencies: {provider0: '*'}}
  package20@2.0.0: {resolution: {integrity: sha512-h}, peerDependencies: {provider0: '*'}}
  package26@1.0.0: {resolution: {integrity: sha512-i}}
  package26@2.0.0: {resolution: {integrity: sha512-j}}
  package27@1.0.0: {resolution: {integrity: sha512-k}}
  package27@2.0.0: {resolution: {integrity: sha512-l}}
  provider0@1.0.0: {resolution: {integrity: sha512-m}}
  provider0@2.0.0: {resolution: {integrity: sha512-n}}
  provider2@1.0.0: {resolution: {integrity: sha512-o}}
  provider2@2.0.0: {resolution: {integrity: sha512-p}}
snapshots:
  package02@1.0.0:
    dependencies: {package10: 1.0.0(provider2@1.0.0), provider2: 1.0.0}
  package02@2.0.0: {}
  package10@1.0.0(provider2@1.0.0):
    dependencies: {package13: 2.0.0, provider2: 1.0.0}
  package13@1.0.0:
    dependencies: {package14: 1.0.0}
  package13@2.0.0: {}
  package14@1.0.0:
    dependencies: {package20: 1.0.0(provider0@1.0.0), provider0: 1.0.0, provider2: 2.0.0}
  package20@1.0.0(provider0@1.0.0):
    dependencies: {provider0: 1.0.0}
  package20@2.0.0(provider0@2.0.0):
    dependencies: {package26: 2.0.0, package27: 2.0.0, provider0: 2.0.0}
  package26@1.0.0: {}
  package26@2.0.0:
    dependencies: {package27: 1.0.0}
  package27@1.0.0:
    dependencies: {provider2: 2.0.0}
  package27@2.0.0: {}
  provider0@1.0.0: {}
  provider0@2.0.0: {}
  provider2@1.0.0: {}
  provider2@2.0.0: {}
",
    );
    let project = child_of(&result, "packages%2Fproject2");
    child_of(&project, "provider2");
    let parent = child_of(&project, "package02");
    assert_eq!(names(&parent), ["package10", "package13"], "{result:#?}");
}

/// `package38@1` depends on `package39@2` and is nested in `package32@1`.
/// `packages/project3` uses `package39@1`, so `package39@2` goes no higher
/// than `package20`, the parent of `package32`. `package32` may not move up
/// to the project after that: `package38` would find `package39@1` from
/// there. [`hoist`] checks that. The lockfile is a generated one, reduced
/// to what the layout needs.
#[test]
fn a_package_stays_below_a_dependency_hoisted_out_of_a_package_nested_in_it() {
    let result = hoist_lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      package24: {specifier: '*', version: 2.0.0}
  packages/project3:
    dependencies:
      package20: {specifier: '*', version: 1.0.0(provider0@2.0.0)}
      package39: {specifier: '*', version: 1.0.0}
      provider0: {specifier: '*', version: 2.0.0}
  packages/project4:
    dependencies:
      package10: {specifier: '*', version: 2.0.0(provider2@2.0.0)}
      provider0: {specifier: '*', version: 1.0.0}
      provider2: {specifier: '*', version: 2.0.0}
packages:
  package10@2.0.0: {resolution: {integrity: sha512-a}, peerDependencies: {provider2: '*'}}
  package20@1.0.0: {resolution: {integrity: sha512-b}, peerDependencies: {provider0: '*'}}
  package24@1.0.0: {resolution: {integrity: sha512-c}}
  package24@2.0.0: {resolution: {integrity: sha512-d}}
  package27@1.0.0: {resolution: {integrity: sha512-e}}
  package31@1.0.0: {resolution: {integrity: sha512-f}}
  package31@2.0.0: {resolution: {integrity: sha512-g}}
  package32@1.0.0: {resolution: {integrity: sha512-h}}
  package32@2.0.0: {resolution: {integrity: sha512-i}}
  package38@1.0.0: {resolution: {integrity: sha512-j}}
  package38@2.0.0: {resolution: {integrity: sha512-k}}
  package39@1.0.0: {resolution: {integrity: sha512-l}}
  package39@2.0.0: {resolution: {integrity: sha512-m}}
  provider0@1.0.0: {resolution: {integrity: sha512-n}}
  provider0@2.0.0: {resolution: {integrity: sha512-o}}
  provider2@2.0.0: {resolution: {integrity: sha512-p}}
snapshots:
  package10@2.0.0(provider2@2.0.0):
    dependencies: {package20: 1.0.0(provider0@1.0.0), provider0: 1.0.0, provider2: 2.0.0}
  package20@1.0.0(provider0@1.0.0):
    dependencies: {package24: 1.0.0, package31: 1.0.0, provider0: 1.0.0}
  package20@1.0.0(provider0@2.0.0):
    dependencies: {package24: 1.0.0, package31: 1.0.0, provider0: 2.0.0}
  package24@1.0.0:
    dependencies: {package31: 2.0.0}
  package24@2.0.0:
    dependencies: {package27: 1.0.0}
  package27@1.0.0:
    dependencies: {package31: 2.0.0}
  package31@1.0.0:
    dependencies: {package32: 1.0.0, package38: 2.0.0}
  package31@2.0.0:
    dependencies: {package32: 2.0.0}
  package32@1.0.0:
    dependencies: {package38: 1.0.0}
  package32@2.0.0: {}
  package38@1.0.0:
    dependencies: {package39: 2.0.0}
  package38@2.0.0: {}
  package39@1.0.0: {}
  package39@2.0.0: {}
  provider0@1.0.0: {}
  provider0@2.0.0: {}
  provider2@2.0.0: {}
",
    );
    let parent = child_of(&child_of(&result, "packages%2Fproject3"), "package20");
    assert_eq!(names(&parent), ["package31", "package32", "package39"], "{result:#?}");
}
