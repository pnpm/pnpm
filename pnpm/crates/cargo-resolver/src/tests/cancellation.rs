use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    CancelError, IndexDiscovery,
    registry::CRATES_IO_SOURCE,
    resolve_lockfile_until,
    tests::{BAR_INDEX, FOO_INDEX, METADATA},
};

const EMPTY_METADATA: &str = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": []
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;

#[test]
fn missing_names_until_restores_pending_dependency_when_cancelled() {
    let mut discovery = IndexDiscovery::new(METADATA, CRATES_IO_SOURCE).unwrap();
    let err = discovery
        .missing_names_until(&|| true)
        .unwrap_err();
    assert!(err.is::<CancelError>(), "{err}");
    assert_eq!(discovery.missing_names().unwrap(), ["foo"]);

    discovery
        .add_entries(&BTreeMap::from([("foo".to_string(), FOO_INDEX.to_string())]))
        .unwrap();
    let step_err = discovery
        .missing_names_until(&|| true)
        .unwrap_err();
    assert!(step_err.is::<CancelError>(), "{step_err}");

    // Also cancel inside `unified_dependencies` after passing the outer dependency poll:
    // poll 0 is the outer `walk_dependency` check, poll 1 scans the first `foo` version,
    // and poll 2 cancels mid-version scan without corrupting `discovered` or `visited`.
    let mid_polls = AtomicUsize::new(0);
    let mid_err = discovery
        .missing_names_until(&|| mid_polls.fetch_add(1, Ordering::Relaxed) == 2)
        .unwrap_err();
    assert!(mid_err.is::<CancelError>(), "{mid_err}");
    assert_eq!(discovery.missing_names().unwrap(), ["bar"]);
}

#[test]
fn discovery_groups_many_zero_zero_lines_linearly_and_preserves_line_feature_growth() {
    let mut index = String::new();
    for patch in 1..=5_000_u32 {
        let dep = if patch == 5_000 {
            r#"[{"name":"leaf","req":"^1.0","features":[],"optional":false,"default_features":true,"target":null,"kind":null,"registry":null,"package":null}]"#
        } else {
            "[]"
        };
        writeln!(
            index,
            r#"{{"name":"many","vers":"0.0.{patch}","deps":{dep},"cksum":"00","features":{{}},"yanked":false}}"#,
        )
        .unwrap();
    }
    index.push_str(
        r#"{"name":"many","vers":"1.0.0","deps":[{"name":"extra_dep","req":"^1.0","features":[],"optional":true,"default_features":true,"target":null,"kind":null,"registry":null,"package":null}],"cksum":"00","features":{"extra":["dep:extra_dep"]},"yanked":false}"#,
    );
    index.push('\n');
    index.push_str(
        r#"{"name":"many","vers":"1.1.0","deps":[],"cksum":"00","features":{"extra":[]},"yanked":false}"#,
    );
    index.push('\n');
    let metadata = r#"{
      "packages": [{
        "id": "path+file:///workspace#app@0.1.0",
        "name": "app",
        "version": "0.1.0",
        "dependencies": [
          {"name": "many", "source": "registry+https://github.com/rust-lang/crates.io-index", "req": ">=0.0.1, <2.0.0", "uses_default_features": false, "features": []},
          {"name": "many", "source": "registry+https://github.com/rust-lang/crates.io-index", "req": "=1.1.0", "uses_default_features": false, "features": ["extra"]}
        ]
      }],
      "workspace_members": ["path+file:///workspace#app@0.1.0"]
    }"#;

    let mut discovery = IndexDiscovery::new(metadata, CRATES_IO_SOURCE).unwrap();
    discovery
        .add_entries(&BTreeMap::from([("many".to_string(), index)]))
        .unwrap();
    let polls = AtomicUsize::new(0);
    let missing = discovery
        .missing_names_until(&|| {
            polls.fetch_add(1, Ordering::Relaxed);
            false
        })
        .unwrap();
    assert_eq!(missing, ["extra_dep", "leaf"]);
    assert!(
        polls.load(Ordering::Relaxed) < 30_000,
        "expected O(V) poll count (~25k vs 50M quadratic) across 5,000 0.0.x lines, got {}",
        polls.load(Ordering::Relaxed),
    );
}

#[test]
fn resolve_lockfile_until_aborts_during_candidate_registration() {
    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), FOO_INDEX.to_string()),
    ]);
    // 1 crate check + 2 lines for `bar` + 1 crate check + 2 lines for `foo` = 6 polls in
    // `Registry::new_until`, then 1 loop header poll + 1 pending package poll = 8 polls before
    // `register_candidates` visits the 2 candidate versions of `foo`. Poll index 9 cancels on
    // the second candidate version inside `register_candidates`.
    let polls = AtomicUsize::new(0);
    let err = resolve_lockfile_until(METADATA, &files, CRATES_IO_SOURCE, &|| {
        polls.fetch_add(1, Ordering::Relaxed) == 9
    })
    .unwrap_err();
    assert!(err.is::<CancelError>(), "{err}");
    assert_eq!(polls.load(Ordering::Relaxed), 10);
}

#[test]
fn resolve_lockfile_until_aborts_during_pubgrub_solve() {
    // Dependency-free metadata has no index files to parse, no pending packages to register,
    // and no root dependencies to validate after solving, so poll 0 is the loop header and
    // poll 1 is strictly inside `CancellableProvider::should_cancel` during `pubgrub::resolve`.
    let files = BTreeMap::new();
    let polls = AtomicUsize::new(0);
    let err = resolve_lockfile_until(EMPTY_METADATA, &files, CRATES_IO_SOURCE, &|| {
        polls.fetch_add(1, Ordering::Relaxed) >= 1
    })
    .unwrap_err();
    assert!(err.is::<CancelError>(), "{err}");
    assert_eq!(polls.load(Ordering::Relaxed), 2);
}
