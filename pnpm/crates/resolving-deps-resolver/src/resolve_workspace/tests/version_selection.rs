use super::{
    DependencyGroup, HashMap, Mutex, RecordingResolver, Utc, WorkspaceImporter, assert_eq,
    fake_manifest, fake_result, importer_opts, recorded_time, resolve_workspace, workspace_opts,
};
use chrono::TimeZone;

#[tokio::test]
async fn time_based_cutoff_is_newest_direct_publish_plus_one_hour() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            Some("2024-03-01T10:00:00.000Z"),
            serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "sub": "^2.0.0" } }),
        ),
    );
    table.insert(
        ("b".to_string(), "^1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            Some("2024-05-20T08:00:00.000Z"),
            serde_json::json!({ "name": "b", "version": "1.0.0" }),
        ),
    );
    table.insert(
        ("sub".to_string(), "^2.0.0".to_string()),
        fake_result("sub", "2.0.0", None, serde_json::json!({ "name": "sub", "version": "2.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0", "b": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(true, true),
        |_| importer_opts(tmp.path().to_path_buf(), None),
    )
    .await
    .unwrap();

    let expected_cutoff = Utc.with_ymd_and_hms(2024, 5, 20, 9, 0, 0).unwrap();
    assert_eq!(
        resolver.opts_for("a"),
        (true, None),
        "direct deps pick lowest under maximum (none)",
    );
    assert_eq!(resolver.opts_for("b"), (true, None));
    assert_eq!(
        resolver.opts_for("sub"),
        (false, Some(expected_cutoff)),
        "subdep picks highest, constrained to newest-direct + 1h",
    );
    assert_eq!(
        result.time,
        recorded_time(&[
            ("a@1.0.0", "2024-03-01T10:00:00.000Z"),
            ("b@1.0.0", "2024-05-20T08:00:00.000Z"),
        ]),
        "the direct deps' publish dates are handed to the lockfile's `time:` section",
    );
}

#[tokio::test]
async fn time_based_cutoff_is_clamped_by_minimum_release_age() {
    let maximum = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            Some("2024-05-20T08:00:00.000Z"),
            serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "sub": "^2.0.0" } }),
        ),
    );
    table.insert(
        ("sub".to_string(), "^2.0.0".to_string()),
        fake_result("sub", "2.0.0", None, serde_json::json!({ "name": "sub", "version": "2.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(true, true),
        |_| importer_opts(tmp.path().to_path_buf(), Some(maximum)),
    )
    .await
    .unwrap();

    assert_eq!(
        resolver.opts_for("a"),
        (true, Some(maximum)),
        "direct deps use the minimumReleaseAge cutoff",
    );
    assert_eq!(
        resolver.opts_for("sub"),
        (false, Some(maximum)),
        "the later time-based candidate is clamped to the minimumReleaseAge cutoff",
    );
}
