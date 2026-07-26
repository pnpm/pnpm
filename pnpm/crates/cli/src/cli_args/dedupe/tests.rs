use pacquet_package_manager::{LockfileDiff, SnapshotDiff};

use super::render_dedupe_check_issues;

#[test]
fn a_resolution_free_rewrite_says_so() {
    let report = render_dedupe_check_issues(&LockfileDiff::default());
    assert_eq!(
        report,
        "The lockfile would be rewritten, but no dependency resolution would change.",
    );
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
