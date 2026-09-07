use std::{collections::HashSet, sync::Mutex};

use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_reporter::{LogEvent, Reporter};

use super::report_peer_dependency_issues;
use crate::InstallError;

#[test]
fn only_resolver_issue_candidates_are_walked() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      peer-user:
        specifier: 1.0.0
        version: 1.0.0
packages:
  peer-user@1.0.0:
    resolution:
      integrity: sha512-deadbeef
    peerDependencies:
      peer-provider: ^1.0.0
snapshots:
  peer-user@1.0.0: {}
",
    )
    .expect("parse lockfile fixture");
    let lockfile_dir = tempfile::tempdir().expect("create lockfile directory");
    let mut config = Config::new();
    config.strict_peer_dependencies = true;
    let catalogs = Catalogs::new();

    report_peer_dependency_issues::<RecordingReporter>(
        Some(&lockfile),
        &HashSet::new(),
        &HashSet::from([".".to_string()]),
        lockfile_dir.path(),
        &config,
        Some(&catalogs),
    )
    .expect("a clean resolver candidate set must skip the lockfile walk");
    assert_eq!(EVENTS.lock().unwrap().len(), 0);

    let error = report_peer_dependency_issues::<RecordingReporter>(
        Some(&lockfile),
        &HashSet::from([".".to_string()]),
        &HashSet::from([".".to_string()]),
        lockfile_dir.path(),
        &config,
        Some(&catalogs),
    )
    .expect_err("a resolver candidate with a missing peer must fail in strict mode");
    assert!(matches!(error, InstallError::PeerDependencyIssues));
    let events = EVENTS.lock().unwrap();
    assert!(matches!(events.as_slice(), [LogEvent::Global(_)]), "unexpected events: {events:?}");
}
