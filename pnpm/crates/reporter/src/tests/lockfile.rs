use super::{
    Envelope, LockfileVerificationLog, LockfileVerificationMessage, LogEvent, LogLevel, Pipe,
    SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalParent,
    SkippedOptionalReason, Value, assert_eq,
};

/// `resolution_failure` payload uses the second `package` variant:
/// no `id`, optional `name` / `version`, and a `bareSpecifier`
/// (camelCase on the wire). The resolver-time emit carries whatever
/// fields the resolver had at fail time — bare specifier always
/// present; `name` / `version` only when the resolver advanced far
/// enough to extract them.
#[test]
fn skipped_optional_resolution_failure_event_matches_pnpm_wire_shape() {
    let event = LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some("ERR_PNPM_FETCH_404: tarball not found".to_string()),
        package: SkippedOptionalPackage::ResolutionFailure {
            name: Some("foo".to_string()),
            version: Some("1.2.3".to_string()),
            bare_specifier: "^1.2.0".to_string(),
        },
        parents: Some(vec![SkippedOptionalParent {
            id: "parent@2.0.0".to_string(),
            name: "parent".to_string(),
            version: "2.0.0".to_string(),
        }]),
        prefix: "/projects/x".to_string(),
        reason: SkippedOptionalReason::ResolutionFailure,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["name"], "pnpm:skipped-optional-dependency");
    assert_eq!(json["reason"], "resolution_failure");
    assert!(json["package"].get("id").is_none(), "id must NOT be present on resolution_failure");
    assert_eq!(json["package"]["name"], "foo");
    assert_eq!(json["package"]["version"], "1.2.3");
    assert_eq!(json["package"]["bareSpecifier"], "^1.2.0");
    assert_eq!(
        json["parents"],
        serde_json::json!([{ "id": "parent@2.0.0", "name": "parent", "version": "2.0.0" }]),
    );
}

/// `name` and `version` are upstream-optional on the
/// resolution-failure variant and must be omitted from the wire
/// when absent. `bareSpecifier` is required.
#[test]
fn skipped_optional_resolution_failure_omits_absent_name_and_version() {
    let event = LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: None,
        package: SkippedOptionalPackage::ResolutionFailure {
            name: None,
            version: None,
            bare_specifier: "git+ssh://broken-url".to_string(),
        },
        parents: Some(Vec::new()),
        prefix: "/projects/y".to_string(),
        reason: SkippedOptionalReason::ResolutionFailure,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert!(json["package"].get("name").is_none(), "name omitted when absent, got {json:?}");
    assert!(json["package"].get("version").is_none(), "version omitted when absent, got {json:?}");
    assert_eq!(json["package"]["bareSpecifier"], "git+ssh://broken-url");
    assert_eq!(json["parents"], serde_json::json!([]), "an empty chain serializes as []");
}

/// `pnpm:lockfile-verification` `started` event carries `entries` and
/// the camelCase `lockfilePath`, both flattened into the envelope
/// alongside `status: "started"`.
#[test]
fn lockfile_verification_started_event_matches_pnpm_wire_shape() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Started {
            entries: 12,
            lockfile_path: Some("/proj/pnpm-lock.yaml".to_string()),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:lockfile-verification");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["status"], "started");
    assert_eq!(json["entries"], 12);
    assert_eq!(json["lockfilePath"], "/proj/pnpm-lock.yaml");
    assert!(json.get("elapsedMs").is_none(), "elapsedMs must be absent on started");
}

/// `pnpm:lockfile-verification` `progress` event carries the running
/// checked count with `status: "progress"`.
#[test]
fn lockfile_verification_progress_event_matches_pnpm_wire_shape() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Progress {
            entries: 12,
            checked: 7,
            lockfile_path: Some("/proj/pnpm-lock.yaml".to_string()),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:lockfile-verification");
    assert_eq!(json["status"], "progress");
    assert_eq!(json["entries"], 12);
    assert_eq!(json["checked"], 7);
    assert_eq!(json["lockfilePath"], "/proj/pnpm-lock.yaml");
    assert!(json.get("elapsedMs").is_none(), "elapsedMs must be absent on progress");
}

/// `pnpm:lockfile-verification` `done` event adds the checked count
/// and `elapsedMs` in camelCase, with `status: "done"`.
#[test]
fn lockfile_verification_done_event_matches_pnpm_wire_shape() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Done {
            entries: 12,
            checked: 12,
            elapsed_ms: 234,
            lockfile_path: Some("/proj/pnpm-lock.yaml".to_string()),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:lockfile-verification");
    assert_eq!(json["status"], "done");
    assert_eq!(json["entries"], 12);
    assert_eq!(json["checked"], 12);
    assert_eq!(json["elapsedMs"], 234);
    assert_eq!(json["lockfilePath"], "/proj/pnpm-lock.yaml");
}

/// `pnpm:lockfile-verification` `failed` mirrors the `done` shape
/// except for the discriminator. Upstream sends it whenever the gate
/// emitted `started` but didn't reach `done` — policy violations and
/// unexpected throws alike — so the reporter can close out the
/// transient frame.
#[test]
fn lockfile_verification_failed_event_matches_pnpm_wire_shape() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Failed {
            entries: 12,
            checked: 0,
            elapsed_ms: 999,
            lockfile_path: Some("/proj/pnpm-lock.yaml".to_string()),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["entries"], 12);
    assert_eq!(json["checked"], 0);
    assert_eq!(json["elapsedMs"], 999);
    assert_eq!(json["lockfilePath"], "/proj/pnpm-lock.yaml");
}

/// `pnpm:lockfile-verification` `cached` carries the discriminator,
/// the camelCase `verifiedAt` of the reused verdict, and the optional
/// camelCase `lockfilePath` — no `entries` or `elapsedMs`, because
/// the cache short-circuit happens before candidates are collected.
#[test]
fn lockfile_verification_cached_event_matches_pnpm_wire_shape() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Cached {
            verified_at: Some("2026-06-11T10:00:00.000Z".to_string()),
            lockfile_path: Some("/proj/pnpm-lock.yaml".to_string()),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:lockfile-verification");
    assert_eq!(json["status"], "cached");
    assert_eq!(json["verifiedAt"], "2026-06-11T10:00:00.000Z");
    assert_eq!(json["lockfilePath"], "/proj/pnpm-lock.yaml");
    assert!(json.get("entries").is_none(), "entries must be absent on cached");
    assert!(json.get("elapsedMs").is_none(), "elapsedMs must be absent on cached");
}

/// A cached record written before `verifiedAt` existed surfaces as
/// `None` and must be omitted from the wire rather than rendered as
/// `null` — pnpm's reporter falls back to a timeless message on
/// absence.
#[test]
fn lockfile_verification_cached_omits_absent_verified_at() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Cached { verified_at: None, lockfile_path: None },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert!(
        json.get("verifiedAt").is_none(),
        "verifiedAt must be omitted when absent, got {json:?}",
    );
}

/// `lockfilePath` is upstream-optional (undefined in test paths that
/// skip the cache wiring). When `None`, the field must be omitted
/// rather than rendered as `null` — pnpm's reporter dispatches on
/// presence to decide whether to render the path suffix.
#[test]
fn lockfile_verification_omits_absent_lockfile_path() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Started { entries: 1, lockfile_path: None },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert!(
        json.get("lockfilePath").is_none(),
        "lockfilePath must be omitted when absent, got {json:?}",
    );
}
