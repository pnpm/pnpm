use std::{collections::HashSet, sync::Mutex};

use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, Reporter};

use super::{PeerReportWorkspace, report_peer_dependency_issues};
use crate::InstallError;

#[test]
fn linked_non_json_manifests_are_checked_during_install() {
    for basename in ["package.json5", "package.yaml"] {
        eprintln!("manifest format: {basename}");
        let temp = tempfile::tempdir().unwrap();
        let lib_dir = temp.path().join("lib");
        std::fs::create_dir(&lib_dir).unwrap();
        let manifest = PackageManifest::from_value(
            lib_dir.join(basename),
            serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "peerDependencies": { "provider": "^1.0.0" },
            }),
        );
        let manifests = [(lib_dir, &manifest)];
        let lockfile = serde_json::from_value(serde_json::json!({
            "lockfileVersion": "9.0",
            "importers": {
                ".": { "dependencies": {
                    "lib": { "specifier": "workspace:*", "version": "link:lib" },
                } },
                "lib": {},
            },
        }))
        .expect("parse lockfile");
        let mut config = Config::new();
        config.strict_peer_dependencies = true;
        let importers = HashSet::from([".".to_string()]);
        let error = report_peer_dependency_issues::<pnpm_reporter::SilentReporter>(
            Some(&lockfile),
            &importers,
            &importers,
            PeerReportWorkspace {
                lockfile_dir: temp.path(),
                catalogs: None,
                project_manifests: &manifests,
            },
            &config,
        )
        .expect_err("a linked project's missing peer must fail in strict mode");
        eprintln!("{error:?}");
        assert!(matches!(error, InstallError::PeerDependencyIssues { .. }));
    }
}

#[test]
fn only_resolver_issue_candidates_are_walked() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
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
    let workspace = PeerReportWorkspace {
        lockfile_dir: lockfile_dir.path(),
        catalogs: Some(&catalogs),
        project_manifests: &[],
    };

    report_peer_dependency_issues::<RecordingReporter>(
        Some(&lockfile),
        &HashSet::new(),
        &HashSet::from([".".to_string()]),
        workspace,
        &config,
    )
    .expect("a clean resolver candidate set must skip the lockfile walk");
    assert_eq!(EVENTS.lock().unwrap().len(), 0);

    let error = report_peer_dependency_issues::<RecordingReporter>(
        Some(&lockfile),
        &HashSet::from([".".to_string()]),
        &HashSet::from([".".to_string()]),
        workspace,
        &config,
    )
    .expect_err("a resolver candidate with a missing peer must fail in strict mode");
    assert!(matches!(error, InstallError::PeerDependencyIssues { rendered: None }));
    let events = EVENTS.lock().unwrap();
    assert!(matches!(events.as_slice(), [LogEvent::Global(_)]), "unexpected events: {events:?}");
}
