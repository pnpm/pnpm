use std::path::Path;

use pretty_assertions::assert_eq;
use serde_json::json;

use super::{FilterLockfileOptions, LockfileKind, filter_lockfile_by_importers, lockfile_path};

const LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      is-odd:
        specifier: 3.0.1
        version: 3.0.1

  packages/other:
    dependencies:
      unrelated:
        specifier: 1.0.0
        version: 1.0.0

packages:

  is-number@6.0.0:
    resolution: {integrity: sha512-number}

  is-odd@3.0.1:
    resolution: {integrity: sha512-odd}

  unrelated@1.0.0:
    resolution: {integrity: sha512-unrelated}

snapshots:

  is-number@6.0.0: {}

  is-odd@3.0.1:
    dependencies:
      is-number: 6.0.0

  unrelated@1.0.0: {}

bit:
  depsRequiringBuild:
    - esbuild@0.25.0
";

fn lockfile_json() -> serde_json::Value {
    let lockfile = pnpm_lockfile::Lockfile::parse(LOCKFILE, Path::new("pnpm-lock.yaml"))
        .expect("parse lockfile")
        .expect("lockfile is not empty");
    serde_json::to_value(lockfile).expect("serialize lockfile")
}

#[test]
fn the_wanted_lockfile_sits_at_the_root_and_the_current_one_under_the_virtual_store() {
    assert_eq!(
        lockfile_path("/repo", None, &LockfileKind::Wanted),
        Path::new("/repo/pnpm-lock.yaml"),
    );
    assert_eq!(
        lockfile_path("/repo", None, &LockfileKind::Current),
        Path::new("/repo/node_modules/.pnpm/lock.yaml"),
    );
}

/// A relative `modulesDir` resolves against the lockfile directory, an
/// absolute one is used as given.
#[test]
fn a_modules_dir_override_relocates_the_current_lockfile() {
    assert_eq!(
        lockfile_path("/repo", Some("nm"), &LockfileKind::Current),
        Path::new("/repo/nm/.pnpm/lock.yaml"),
    );
    assert_eq!(
        lockfile_path("/repo", Some("/elsewhere/nm"), &LockfileKind::Current),
        Path::new("/elsewhere/nm/.pnpm/lock.yaml"),
    );
}

#[test]
fn an_unknown_lockfile_kind_is_rejected() {
    let error = LockfileKind::parse(Some("previous")).expect_err("unknown kind");

    assert!(error.reason.contains("unknown lockfile kind"), "{}", error.reason);
}

/// The JSON crossing the boundary is the lockfile file's own shape: each
/// importer dependency an `{ specifier, version }` pair, `packages` and
/// `snapshots` separate.
#[test]
fn the_json_shape_is_the_lockfile_files_own() {
    let lockfile = lockfile_json();

    assert_eq!(lockfile["importers"]["."]["dependencies"]["is-odd"]["specifier"], "3.0.1");
    assert_eq!(lockfile["importers"]["."]["dependencies"]["is-odd"]["version"], "3.0.1");
    assert!(lockfile["packages"]["is-odd@3.0.1"].is_object());
    assert!(lockfile["snapshots"]["is-odd@3.0.1"].is_object());
}

/// A host's own top-level block survives the round trip, so it can read the
/// lockfile, edit its block, and write the file back without pnpm deleting
/// the rest of what it recorded.
#[test]
fn foreign_top_level_keys_survive_the_boundary() {
    let lockfile = lockfile_json();

    assert_eq!(lockfile["bit"]["depsRequiringBuild"][0], "esbuild@0.25.0");
}

#[test]
fn filtering_keeps_only_what_the_named_importer_reaches() {
    let filtered = filter_lockfile_by_importers(lockfile_json(), vec![".".to_string()], None)
        .expect("filter lockfile");

    assert!(filtered["snapshots"]["is-odd@3.0.1"].is_object());
    assert!(filtered["snapshots"]["is-number@6.0.0"].is_object());
    assert!(filtered["snapshots"]["unrelated@1.0.0"].is_null());
    assert!(filtered["packages"]["unrelated@1.0.0"].is_null());
}

#[test]
fn a_skipped_dep_path_is_dropped_along_with_what_only_it_reaches() {
    let options = FilterLockfileOptions {
        include_dependencies: None,
        include_dev_dependencies: None,
        include_optional_dependencies: None,
        skipped: Some(vec!["is-odd@3.0.1".to_string()]),
        fail_on_missing_dependencies: None,
    };

    let filtered =
        filter_lockfile_by_importers(lockfile_json(), vec![".".to_string()], Some(options))
            .expect("filter lockfile");

    assert!(filtered["snapshots"]["is-odd@3.0.1"].is_null());
    assert!(filtered["snapshots"]["is-number@6.0.0"].is_null());
}

/// An unparsable entry in a host's skip list matches no snapshot key
/// either way, so it is ignored rather than failing the call.
#[test]
fn an_unparsable_skipped_entry_is_ignored() {
    let options = FilterLockfileOptions {
        include_dependencies: None,
        include_dev_dependencies: None,
        include_optional_dependencies: None,
        skipped: Some(vec![String::new()]),
        fail_on_missing_dependencies: None,
    };

    let filtered =
        filter_lockfile_by_importers(lockfile_json(), vec![".".to_string()], Some(options))
            .expect("filter lockfile");

    assert!(filtered["snapshots"]["is-odd@3.0.1"].is_object());
}

#[test]
fn filtering_rejects_a_value_that_is_not_a_lockfile() {
    let error = filter_lockfile_by_importers(json!({ "nope": true }), vec![], None)
        .expect_err("not a lockfile");

    assert!(error.reason.contains("is not a lockfile"), "{}", error.reason);
}
