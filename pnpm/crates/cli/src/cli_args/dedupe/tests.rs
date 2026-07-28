use std::{collections::HashSet, marker::PhantomData, sync::Mutex};

use pacquet_lockfile::PackageMetadata;
use pacquet_package_manager::{InstallabilityHost, LockfileDiff, SnapshotDiff};
use pacquet_reporter::{LogEvent, ProgressMessage, Reporter};
use pacquet_store_dir::{PackageFilesIndex, StoreDir, StoreIndex, store_index_key};
use tempfile::TempDir;

use super::{
    DedupeResolutionReporter, emit_dedupe_check_error, render_dedupe_check_error,
    render_dedupe_check_issues, reusable_skipped_package_id,
};

#[test]
fn a_resolution_free_rewrite_says_so() {
    let report = render_dedupe_check_issues(&LockfileDiff::default());
    assert_eq!(
        report,
        "The lockfile would be rewritten, but no dependency resolution would change.",
    );
}

#[test]
fn check_error_matches_pnpm_reporter_format() {
    let report = render_dedupe_check_error(&LockfileDiff::default());
    eprintln!("REPORT:\n{report}\n");
    assert_eq!(
        report,
        "\
[ERR_PNPM_DEDUPE_CHECK_ISSUES] Dedupe --check found changes to the lockfile

The lockfile would be rewritten, but no dependency resolution would change.

Run pnpm dedupe to apply the changes above.
",
    );
}

#[test]
fn check_error_emits_the_structured_pnpm_event() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    emit_dedupe_check_error::<RecordingReporter>(&LockfileDiff::default());

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::DedupeCheck(log)]
                if log.err.code == "ERR_PNPM_DEDUPE_CHECK_ISSUES"
                    && log.dedupe_check_issues["importerIssuesByImporterId"]["updated"]
                        == serde_json::json!({})
        ),
        "unexpected events: {captured:?}",
    );
}

#[test]
fn resolution_observer_emits_resolved_progress() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let observer = DedupeResolutionReporter::<RecordingReporter> {
        requester: "/project".to_string(),
        store_index: None,
        reusable_skipped_package_ids: HashSet::new(),
        reporter: PhantomData,
    };
    pacquet_package_manager::ResolutionObserver::on_resolved(&observer, resolved_dep_hint());

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::Progress(log)]
                if matches!(
                    &log.message,
                    ProgressMessage::Resolved { package_id, requester }
                        if package_id == "dep@2.0.0" && requester == "/project"
                )
        ),
        "unexpected events: {captured:?}",
    );
}

#[test]
fn resolution_observer_reports_packages_found_in_store() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let root = TempDir::new().unwrap();
    let store_dir = StoreDir::new(root.path());
    std::fs::create_dir_all(store_dir.root()).unwrap();
    StoreIndex::open_in(&store_dir)
        .unwrap()
        .set(&store_index_key("sha512-test", "dep@2.0.0"), &PackageFilesIndex::default())
        .unwrap();
    let observer = DedupeResolutionReporter::<RecordingReporter> {
        requester: "/project".to_string(),
        store_index: StoreIndex::shared_readonly_in(&store_dir),
        reusable_skipped_package_ids: HashSet::new(),
        reporter: PhantomData,
    };

    pacquet_package_manager::ResolutionObserver::on_resolved(&observer, resolved_dep_hint());

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [
                LogEvent::Progress(resolved),
                LogEvent::Progress(found_in_store),
            ] if matches!(&resolved.message, ProgressMessage::Resolved { .. })
                && matches!(&found_in_store.message, ProgressMessage::FoundInStore { .. })
        ),
        "unexpected events: {captured:?}",
    );
}

#[test]
fn resolution_observer_reports_skipped_packages_as_reused() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let observer = DedupeResolutionReporter::<RecordingReporter> {
        requester: "/project".to_string(),
        store_index: None,
        reusable_skipped_package_ids: HashSet::from(["dep@2.0.0".to_string()]),
        reporter: PhantomData,
    };
    pacquet_package_manager::ResolutionObserver::on_resolved(&observer, resolved_dep_hint());

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [
                LogEvent::Progress(resolved),
                LogEvent::Progress(found_in_store),
            ] if matches!(&resolved.message, ProgressMessage::Resolved { .. })
                && matches!(&found_in_store.message, ProgressMessage::FoundInStore { .. })
        ),
        "unexpected events: {captured:?}",
    );
}

#[test]
fn engine_incompatible_skipped_packages_are_not_reported_as_reused() {
    let package_key = "engine-constrained@1.0.0".parse().unwrap();
    let metadata: PackageMetadata = serde_json::from_value(serde_json::json!({
        "resolution": {
            "integrity": "sha512-dGVzdA==",
        },
        "engines": {
            "node": "<1",
        },
    }))
    .unwrap();
    let host = InstallabilityHost {
        node_version: "22.0.0".to_string(),
        node_detected: true,
        os: "linux",
        cpu: "x64",
        libc: "glibc",
        supported_architectures: None,
        engine_strict: false,
    };

    let reusable = reusable_skipped_package_id(&package_key, &metadata, &host, None).unwrap();
    assert!(reusable.is_none());
}

fn resolved_dep_hint() -> pacquet_package_manager::ResolvedPackageHint<'static> {
    pacquet_package_manager::ResolvedPackageHint {
        id: "dep@2.0.0",
        name: "dep",
        version: "2.0.0",
        integrity: "sha512-test",
        tarball_url: "https://registry.example/dep/-/dep-2.0.0.tgz",
        unpacked_size: None,
        file_count: None,
        from_registry: true,
    }
}

/// The report pnpm's `renderDedupeCheckIssues` produces: a tree per changed
/// importer, then the snapshots deduplication adds and drops.
#[test]
fn changes_render_under_importer_and_package_headings() {
    let diff = LockfileDiff {
        importers: vec![SnapshotDiff {
            updated: vec![(
                "@babel/core".to_string(),
                "7.26.10".to_string(),
                "7.26.10(supports-color@9.4.0)".to_string(),
            )],
            ..snapshot_diff(".")
        }],
        added_packages: vec!["supports-color@9.4.0".to_string()],
        removed_packages: vec!["ws@8.14.2".to_string()],
        updated_packages: vec![SnapshotDiff {
            added: vec![("supports-color".to_string(), "9.4.0".to_string())],
            removed: vec![("debug".to_string(), "4.3.4".to_string())],
            ..snapshot_diff("chalk@5.3.0")
        }],
    };

    let report = render_dedupe_check_issues(&diff);
    eprintln!("REPORT:\n{report}\n");

    assert_eq!(
        report,
        "\
Importers
.
└── @babel/core 7.26.10 → 7.26.10(supports-color@9.4.0)


Packages
chalk@5.3.0
├── + supports-color 9.4.0
└── - debug 4.3.4

+ supports-color@9.4.0
- ws@8.14.2
",
    );
}

/// An importer-only diff leaves the `Packages` heading out entirely, rather
/// than printing an empty section.
#[test]
fn an_empty_section_is_omitted() {
    let diff = LockfileDiff {
        importers: vec![SnapshotDiff {
            added: vec![("is-positive".to_string(), "1.0.0".to_string())],
            ..snapshot_diff(".")
        }],
        ..LockfileDiff::default()
    };

    let report = render_dedupe_check_issues(&diff);
    assert!(report.starts_with("Importers\n"), "got: {report}");
    assert!(!report.contains("Packages"), "got: {report}");
}

fn snapshot_diff(id: &str) -> SnapshotDiff {
    SnapshotDiff { id: id.to_string(), added: Vec::new(), removed: Vec::new(), updated: Vec::new() }
}
