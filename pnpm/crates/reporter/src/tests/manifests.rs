use super::{
    Envelope, LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Pipe, Value,
    assert_eq,
};

/// `pnpm:package-manifest` is presence-tagged on `initial` /
/// `updated`. The JS reporter checks `'initial' in log` to dispatch,
/// so the wire shape must carry exactly one of the two keys (never
/// both, never neither). The payload value is the entire
/// `package.json` body — pnpm threads it through unchanged.
#[test]
fn package_manifest_event_matches_pnpm_wire_shape() {
    let manifest = serde_json::json!({
        "name": "demo",
        "version": "1.0.0",
        "dependencies": { "fastify": "1.0.0" },
    });

    let event = LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Initial {
            prefix: "/proj".to_string(),
            initial: manifest.clone(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:package-manifest");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["prefix"], "/proj");
    assert_eq!(json["initial"], manifest);
    assert!(json.get("updated").is_none(), "initial event must not carry updated");

    let event = LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated {
            prefix: "/proj".to_string(),
            updated: manifest.clone(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["updated"], manifest);
    assert!(json.get("initial").is_none(), "updated event must not carry initial");
}
