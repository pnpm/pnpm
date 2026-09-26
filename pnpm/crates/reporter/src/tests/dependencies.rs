use super::{
    Envelope, LogEvent, LogLevel, PeerDependencyIssuesLog, Pipe, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalReason, Value, assert_eq,
};

/// `pnpm:skipped-optional-dependency` matches pnpm's wire shape:
/// top-level `details`, `package: { id, name, version }`, `prefix`,
/// and `reason` (`snake_case`).
#[test]
fn skipped_optional_dependency_event_matches_pnpm_wire_shape() {
    let event = LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some("build failed: exit code 1".to_string()),
        package: SkippedOptionalPackage::Installed {
            id: "/foo/1.0.0".to_string(),
            name: "foo".to_string(),
            version: "1.0.0".to_string(),
        },
        parents: None,
        prefix: "/projects/x".to_string(),
        reason: SkippedOptionalReason::BuildFailure,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["name"], "pnpm:skipped-optional-dependency");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["reason"], "build_failure");
    assert_eq!(json["details"], "build failed: exit code 1");
    assert_eq!(json["prefix"], "/projects/x");
    assert_eq!(json["package"]["id"], "/foo/1.0.0");
    assert_eq!(json["package"]["name"], "foo");
    assert_eq!(json["package"]["version"], "1.0.0");
    assert!(json.get("parents").is_none(), "non-resolver emits carry no parents, got {json:?}");
}

/// `details` is optional upstream and must be omitted from the wire
/// when absent (`skip_serializing_if = "Option::is_none"`).
#[test]
fn skipped_optional_omits_absent_details() {
    let event = LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: None,
        package: SkippedOptionalPackage::Installed {
            id: "/bar/2.0.0".to_string(),
            name: "bar".to_string(),
            version: "2.0.0".to_string(),
        },
        parents: None,
        prefix: "/projects/y".to_string(),
        reason: SkippedOptionalReason::BuildFailure,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert!(json.get("details").is_none(), "details must be omitted when absent, got {json:?}");
}

/// All four reason variants serialize as the `snake_case` strings
/// pnpm's reporter dispatches on.
#[test]
fn skipped_optional_reason_serializes_in_pnpm_form() {
    let cases = [
        (SkippedOptionalReason::BuildFailure, "build_failure"),
        (SkippedOptionalReason::UnsupportedEngine, "unsupported_engine"),
        (SkippedOptionalReason::UnsupportedPlatform, "unsupported_platform"),
        (SkippedOptionalReason::ResolutionFailure, "resolution_failure"),
    ];
    for (reason, expected) in cases {
        let json = serde_json::to_string(&reason).expect("serialize reason");
        assert_eq!(json, format!(r#""{expected}""#), "{reason:?} must serialize as {expected:?}");
    }
}

#[test]
fn peer_dependency_issues_event_matches_pnpm_wire_shape() {
    let event = LogEvent::PeerDependencyIssues(PeerDependencyIssuesLog {
        level: LogLevel::Debug,
        issues_by_projects: serde_json::json!({
            ".": {
                "bad": {
                    "react": [{
                        "parents": [{ "name": "react-inspector", "version": "6.0.2" }],
                        "optional": false,
                        "wantedRange": "^18.0.0",
                        "foundVersion": "19.1.0",
                        "resolvedFrom": [],
                    }],
                },
                "missing": {},
                "conflicts": [],
                "intersections": {},
            },
        }),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:peer-dependency-issues");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["issuesByProjects"]["."]["bad"]["react"][0]["foundVersion"], "19.1.0");
}
