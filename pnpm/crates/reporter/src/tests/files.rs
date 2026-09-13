use super::{Envelope, IgnoredScriptsLog, LogEvent, LogLevel, Pipe, Value, assert_eq};

/// `pnpm:ignored-scripts` carries a single field: `packageNames` (camelCase).
/// Default-reporter needs the camelCase spelling.
#[test]
fn ignored_scripts_event_matches_pnpm_wire_shape() {
    let event = LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: vec!["foo@1.0.0".to_string(), "bar@2.0.0".to_string()],
        strict_dep_builds: true,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["name"], "pnpm:ignored-scripts");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["packageNames"], serde_json::json!(["foo@1.0.0", "bar@2.0.0"]));
    assert!(json.get("strictDepBuilds").is_none());
}
