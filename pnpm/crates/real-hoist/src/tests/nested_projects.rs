//! A project whose directory lies inside another project's. Node.js walks
//! up from it through the enclosing project's `node_modules` before the
//! root's, so what the enclosing project holds shadows the root.

use super::{
    HoistOpts,
    HoisterResult,
    Lockfile,
    Rc,
    assert_eq,
    child_of,
    hoist,
};
use std::{
    collections::BTreeSet,
    path::Path,
};

fn hoist_lockfile(yaml: &str) -> HoisterResult {
    hoist_lockfile_with(yaml, &HoistOpts::default())
}

fn hoist_lockfile_with(yaml: &str, opts: &HoistOpts) -> HoisterResult {
    let lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile is not empty");
    hoist(&lockfile, opts).expect("hoist should succeed")
}

/// The names in `project`'s `node_modules`, each with the snapshot key the
/// directory serves first.
fn held_by(result: &HoisterResult, project: &str) -> Vec<String> {
    let project: Rc<HoisterResult> = child_of(result, project);
    let mut held: Vec<String> = project.dependencies
        .borrow()
        .iter()
        .map(|dep| {
            dep.references
                .borrow()
                .first()
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    held.sort_unstable();
    held
}

/// `a` keeps `x@1` because the root holds `x@2`. From `a/b`, `x` then
/// resolves to `a/node_modules/x`, so the `x@2` that `a/b` and its `y@1`
/// depend on cannot be the root's.
#[test]
fn a_nested_project_keeps_what_the_enclosing_project_shadows() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .: {}
  a:
    dependencies:
      x: {specifier: 1.0.0, version: 1.0.0}
  a/b:
    dependencies:
      x: {specifier: 2.0.0, version: 2.0.0}
      y: {specifier: 1.0.0, version: 1.0.0}
  c:
    dependencies:
      x: {specifier: 2.0.0, version: 2.0.0}
      y: {specifier: 2.0.0, version: 2.0.0}
  d:
    dependencies:
      y: {specifier: 2.0.0, version: 2.0.0}
packages:
  x@1.0.0: {resolution: {integrity: sha512-a}}
  x@2.0.0: {resolution: {integrity: sha512-b}}
  y@1.0.0: {resolution: {integrity: sha512-c}}
  y@2.0.0: {resolution: {integrity: sha512-d}}
snapshots:
  x@1.0.0: {}
  x@2.0.0: {}
  y@1.0.0:
    dependencies: {x: 2.0.0}
  y@2.0.0: {}
",
    );
    assert_eq!(held_by(&result, "a"), ["x@1.0.0"], "{result:#?}");
    assert_eq!(held_by(&result, "a%2Fb"), ["x@2.0.0", "y@1.0.0"], "{result:#?}");
    assert!(child_of(&child_of(&result, "a%2Fb"), "y").dependencies.borrow().is_empty());
}

/// The root holds `z@1`, so the `z@2` of `a/b` stays below it. It stays in
/// `a/b` and does not move to `a`, whose `node_modules/.bin` is not on the
/// path of the scripts of `a/b`.
#[test]
fn a_nested_project_s_dependency_does_not_move_to_the_enclosing_project() {
    let result = hoist_lockfile(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      z: {specifier: 1.0.0, version: 1.0.0}
  a: {}
  a/b:
    dependencies:
      z: {specifier: 2.0.0, version: 2.0.0}
packages:
  z@1.0.0: {resolution: {integrity: sha512-a}}
  z@2.0.0: {resolution: {integrity: sha512-b}}
snapshots:
  z@1.0.0: {}
  z@2.0.0: {}
",
    );
    assert_eq!(held_by(&result, "a"), Vec::<String>::new(), "{result:#?}");
    assert_eq!(held_by(&result, "a%2Fb"), ["z@2.0.0"], "{result:#?}");
}

/// The `workspaces` hoisting limit names every project as a border of the
/// root. `a/b` is not a child of the root, and keeps its dependencies all
/// the same.
#[test]
fn a_nested_project_keeps_its_dependencies_under_the_workspaces_limit() {
    let mut opts = HoistOpts::default();
    let projects = BTreeSet::from(["a".to_string(), "a%2Fb".to_string()]);
    opts.hoisting_limits.insert(".@".to_string(), projects);
    let result = hoist_lockfile_with(
        "lockfileVersion: '9.0'
importers:
  .: {}
  a:
    dependencies:
      x: {specifier: 1.0.0, version: 1.0.0}
  a/b:
    dependencies:
      y: {specifier: 1.0.0, version: 1.0.0}
packages:
  x@1.0.0: {resolution: {integrity: sha512-a}}
  y@1.0.0: {resolution: {integrity: sha512-b}}
snapshots:
  x@1.0.0: {}
  y@1.0.0: {}
",
        &opts,
    );
    assert_eq!(held_by(&result, "a"), ["x@1.0.0"], "{result:#?}");
    assert_eq!(held_by(&result, "a%2Fb"), ["y@1.0.0"], "{result:#?}");
    assert_eq!(result.dependencies.borrow().len(), 2, "{result:#?}");
}
