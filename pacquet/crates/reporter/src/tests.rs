use std::sync::Mutex;

use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use serde_json::Value;

use crate::{
    AddedRoot, BrokenModulesLog, ContextLog, DependencyType, Envelope, FetchingProgressLog,
    FetchingProgressMessage, GetHostName, HookLog, Host, IgnoredScriptsLog, LifecycleLog,
    LifecycleMessage, LifecycleStdio, LockfileVerificationLog, LockfileVerificationMessage,
    LogEvent, LogLevel, PackageImportMethod, PackageImportMethodLog, PackageManifestLog,
    PackageManifestMessage, PnpmLog, ProgressLog, ProgressMessage, RemovedRoot, Reporter,
    RequestRetryError, RequestRetryLog, RootLog, RootMessage, SilentReporter,
    SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalReason, Stage, StageLog,
    StatsLog, StatsMessage, SummaryLog,
};

/// Context log serializes with the camelCase field names
/// `@pnpm/cli.default-reporter` expects (`currentLockfileExists`,
/// `storeDir`, `virtualStoreDir`); `snake_case` names would silently
/// fail to render even though the JSON is structurally valid.
#[test]
fn context_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Context(ContextLog {
        level: LogLevel::Debug,
        current_lockfile_exists: false,
        store_dir: "/store".to_string(),
        virtual_store_dir: "/proj/node_modules/.pacquet".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:context");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["currentLockfileExists"], false);
    assert_eq!(json["storeDir"], "/store");
    assert_eq!(json["virtualStoreDir"], "/proj/node_modules/.pacquet");
}

/// Stage log serializes with the channel name flattened into the
/// envelope alongside `time`, `hostname`, `pid`, and the payload
/// fields. This is the wire shape `@pnpm/cli.default-reporter`
/// consumes; adding a wrapper object would break it.
#[test]
fn stage_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: "/some/project".to_string(),
        stage: Stage::ImportingStarted,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:stage");
    assert_eq!(json["stage"], "importing_started");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["prefix"], "/some/project");
    assert_eq!(json["time"], 1_700_000_000_000_u64);
    assert_eq!(json["hostname"], "host");
    assert_eq!(json["pid"], 4242);
}

/// Summary log serializes with the channel name flattened into the
/// envelope alongside `prefix` and the [bunyan]-envelope `level`.
/// `prefix` is what pnpm's reporter uses to find the matching
/// `pnpm:root` history and render its "+N -M" block.
///
/// [bunyan]: https://github.com/trentm/node-bunyan
#[test]
fn summary_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: "/some/project".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:summary");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["prefix"], "/some/project");
}

/// Generic-channel (`name: "pnpm"`) log carries the bare `pnpm`
/// channel name — without a `:`-suffix — matching the shape pnpm's
/// global logger writes. `@pnpm/cli.default-reporter` routes these
/// records through the "other" stream branch; a typo on the channel
/// name would silently fail to render.
#[test]
fn pnpm_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Lockfile is up to date, resolution step is skipped".to_string(),
        prefix: "/some/project".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm");
    assert_eq!(json["level"], "info");
    assert_eq!(json["message"], "Lockfile is up to date, resolution step is skipped");
    assert_eq!(json["prefix"], "/some/project");
}

/// Hook log (`name: "pnpm:hook"`) carries the `from` / `hook` /
/// `message` / `prefix` fields pnpm's `hookLogger` emits, at the
/// `debug` level the hook-context logger uses. `@pnpm/cli.default-reporter`
/// dispatches on these to attribute the message to its pnpmfile.
#[test]
fn hook_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Hook(HookLog {
        level: LogLevel::Debug,
        from: "/some/project/.pnpmfile.cjs".to_string(),
        hook: "readPackage".to_string(),
        message: "is-positive pinned to 1.0.0".to_string(),
        prefix: "/some/project".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:hook");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["from"], "/some/project/.pnpmfile.cjs");
    assert_eq!(json["hook"], "readPackage");
    assert_eq!(json["message"], "is-positive pinned to 1.0.0");
    assert_eq!(json["prefix"], "/some/project");
}

/// Package-import-method log carries the chosen method as one of
/// pnpm's three lowercase strings; anything else (e.g. the
/// kebab-case `clone-or-copy` that `pacquet_config::PackageImportMethod`
/// deserializes from) would silently fail to render.
#[test]
fn package_import_method_event_matches_pnpm_wire_shape() {
    let event = LogEvent::PackageImportMethod(PackageImportMethodLog {
        level: LogLevel::Debug,
        method: PackageImportMethod::Clone,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:package-import-method");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["method"], "clone");

    for (method, expected) in [
        (PackageImportMethod::Clone, "clone"),
        (PackageImportMethod::Hardlink, "hardlink"),
        (PackageImportMethod::Copy, "copy"),
    ] {
        let json = serde_json::to_string(&method).expect("serialize method");
        assert_eq!(json, format!("\"{expected}\""));
    }
}

/// `pnpm:progress` flattens its `status`-tagged payload into the
/// envelope. The three "store-ish" statuses (`resolved`, `fetched`,
/// `found_in_store`) carry `packageId` and `requester`; `imported`
/// substitutes `method` / `to` with no `packageId`. Mirroring pnpm's
/// shape exactly because the JS reporter's switch on `status` is the
/// dispatch.
#[test]
fn progress_event_matches_pnpm_wire_shape() {
    for (message, expected_status) in [
        (
            ProgressMessage::Resolved {
                package_id: "react@18.0.0".to_string(),
                requester: "/proj".to_string(),
            },
            "resolved",
        ),
        (
            ProgressMessage::Fetched {
                package_id: "react@18.0.0".to_string(),
                requester: "/proj".to_string(),
            },
            "fetched",
        ),
        (
            ProgressMessage::FoundInStore {
                package_id: "react@18.0.0".to_string(),
                requester: "/proj".to_string(),
            },
            "found_in_store",
        ),
    ] {
        let event = LogEvent::Progress(ProgressLog { level: LogLevel::Debug, message });
        let envelope =
            Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

        let json: Value = envelope
            .pipe_ref(serde_json::to_string)
            .expect("serialize envelope")
            .pipe_as_ref(serde_json::from_str)
            .expect("parse JSON");

        assert_eq!(json["name"], "pnpm:progress");
        assert_eq!(json["level"], "debug");
        assert_eq!(json["status"], expected_status);
        assert_eq!(json["packageId"], "react@18.0.0");
        assert_eq!(json["requester"], "/proj");
    }

    let event = LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Imported {
            method: PackageImportMethod::Hardlink,
            requester: "/proj".to_string(),
            to: "/proj/node_modules/.pacquet/react@18.0.0/node_modules/react".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:progress");
    assert_eq!(json["status"], "imported");
    assert_eq!(json["method"], "hardlink");
    assert_eq!(json["requester"], "/proj");
    assert_eq!(json["to"], "/proj/node_modules/.pacquet/react@18.0.0/node_modules/react");
    // `imported` deliberately omits `packageId` — match pnpm's shape
    // so consumers that read `progress.packageId` only on the three
    // store-ish statuses don't trip on a stray field.
    assert!(json.get("packageId").is_none(), "imported must not carry packageId");
}

/// `pnpm:fetching-progress` flattens its two-state `status` enum into
/// the envelope. `started` carries `attempt` / `packageId` / `size`
/// (the `Content-Length`-derived value, serialized as JSON `null`
/// when the response is chunked / unknown); `in_progress` carries the
/// running `downloaded` byte count.
#[test]
fn fetching_progress_event_matches_pnpm_wire_shape() {
    let event = LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: 1,
            package_id: "react@18.0.0".to_string(),
            size: Some(123_456),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:fetching-progress");
    assert_eq!(json["status"], "started");
    assert_eq!(json["attempt"], 1);
    assert_eq!(json["packageId"], "react@18.0.0");
    assert_eq!(json["size"], 123_456);

    // Unknown / chunked response: `size` must serialize as JSON null,
    // matching pnpm's `size: number | null` shape. The default-reporter
    // checks `size != null` to decide whether to render a percent
    // gauge; emitting an absent field would silently break that.
    let event = LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: 1,
            package_id: "react@18.0.0".to_string(),
            size: None,
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert!(json.get("size").is_some_and(serde_json::Value::is_null), "size must be JSON null");

    let event = LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::InProgress {
            downloaded: 65_536,
            package_id: "react@18.0.0".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["status"], "in_progress");
    assert_eq!(json["downloaded"], 65_536);
    assert_eq!(json["packageId"], "react@18.0.0");
}

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

/// `pnpm:root` is presence-tagged on `added` / `removed`. The JS
/// reporter accumulates `added` events and renders them in the
/// `pnpm:summary` "+N -M" block. Optional fields skip when absent
/// — emitting them as JSON `null` would put `null` in the rendered
/// version string.
#[test]
fn root_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: "/proj".to_string(),
            added: AddedRoot {
                name: "fastify".to_string(),
                real_name: "fastify".to_string(),
                version: Some("4.0.0".to_string()),
                dependency_type: Some(DependencyType::Prod),
                id: None,
                latest: None,
                linked_from: None,
            },
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:root");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["prefix"], "/proj");
    assert_eq!(json["added"]["name"], "fastify");
    assert_eq!(json["added"]["realName"], "fastify");
    assert_eq!(json["added"]["version"], "4.0.0");
    assert_eq!(json["added"]["dependencyType"], "prod");
    // Optional fields skip when None so the JS reporter doesn't see
    // `id: null` etc. — pnpm's emit also omits them when absent.
    for k in ["id", "latest", "linkedFrom"] {
        assert!(json["added"].get(k).is_none(), "added.{k} should be absent, got {json:?}");
    }
    assert!(json.get("removed").is_none(), "added event must not carry removed");

    let event = LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Removed {
            prefix: "/proj".to_string(),
            removed: RemovedRoot {
                name: "fastify".to_string(),
                version: None,
                dependency_type: None,
            },
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["removed"]["name"], "fastify");
    assert!(json.get("added").is_none(), "removed event must not carry added");

    for (ty, expected) in [
        (DependencyType::Prod, "prod"),
        (DependencyType::Dev, "dev"),
        (DependencyType::Optional, "optional"),
    ] {
        let json = serde_json::to_string(&ty).expect("serialize dependency type");
        assert_eq!(json, format!("\"{expected}\""));
    }
}

/// `pnpm:stats` is presence-tagged on `added` / `removed`. pnpm
/// emits each from a separate site, so an event carries one or the
/// other — never both. Pacquet currently emits both back-to-back
/// (added from `CreateVirtualStore`, removed from a placeholder)
/// to keep the wire shape consumable until pruning lands.
#[test]
fn stats_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: "/proj".to_string(), added: 42 },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:stats");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["prefix"], "/proj");
    assert_eq!(json["added"], 42);
    assert!(json.get("removed").is_none(), "added event must not carry removed");

    let event = LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Removed { prefix: "/proj".to_string(), removed: 0 },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["removed"], 0);
    assert!(json.get("added").is_none(), "removed event must not carry added");
}

/// `pnpm:request-retry` carries the retry loop's bookkeeping
/// (attempt, maxRetries, timeout-ms-until-next-attempt) and a JS-
/// shaped error object. The default-reporter dispatches on the
/// chain `httpStatusCode ?? status ?? errno ?? code`; absent
/// fields must skip rather than render as JSON `null`, since the
/// `??` chain treats `null` as a present value.
#[test]
fn request_retry_event_matches_pnpm_wire_shape() {
    let event = LogEvent::RequestRetry(RequestRetryLog {
        level: LogLevel::Debug,
        attempt: 1,
        error: RequestRetryError {
            message: "503 Service Unavailable".to_string(),
            http_status_code: Some("503".to_string()),
            status: None,
            errno: None,
            code: None,
        },
        max_retries: 2,
        method: "GET".to_string(),
        timeout: 10_000,
        url: "https://registry.npmjs.org/x/-/x-1.0.0.tgz".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:request-retry");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["attempt"], 1);
    assert_eq!(json["maxRetries"], 2);
    assert_eq!(json["method"], "GET");
    assert_eq!(json["timeout"], 10_000);
    assert_eq!(json["url"], "https://registry.npmjs.org/x/-/x-1.0.0.tgz");
    assert_eq!(json["error"]["message"], "503 Service Unavailable");
    assert_eq!(json["error"]["httpStatusCode"], "503");
    for k in ["status", "errno", "code"] {
        assert!(json["error"].get(k).is_none(), "error.{k} should be absent, got {json:?}");
    }
}

/// `pnpm:lifecycle` is presence-tagged on `script` / `line` / `exitCode`.
/// pnpm's reporter dispatches on which of those is present rather than
/// on a `status` discriminator. The shared fields (`depPath`, `stage`,
/// `wd`) appear on every record. Field names use camelCase
/// (`depPath`, `exitCode`) so `@pnpm/cli.default-reporter` parses them.
#[test]
fn lifecycle_event_matches_pnpm_wire_shape() {
    eprintln!("CASE: Script");
    let event = LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Script {
            dep_path: "/x@1.0.0".to_string(),
            optional: false,
            script: "node build.js".to_string(),
            stage: "postinstall".to_string(),
            wd: "/proj/node_modules/.pacquet/x@1.0.0/node_modules/x".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["name"], "pnpm:lifecycle");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["depPath"], "/x@1.0.0");
    assert_eq!(json["optional"], false);
    assert_eq!(json["script"], "node build.js");
    assert_eq!(json["stage"], "postinstall");
    assert_eq!(json["wd"], "/proj/node_modules/.pacquet/x@1.0.0/node_modules/x");
    for k in ["line", "stdio", "exitCode"] {
        assert!(json.get(k).is_none(), "Script must not carry {k}, got {json:?}");
    }

    eprintln!("CASE: Stdio");
    let event = LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Stdio {
            dep_path: "/x@1.0.0".to_string(),
            line: "hello world".to_string(),
            stage: "postinstall".to_string(),
            stdio: LifecycleStdio::Stdout,
            wd: "/wd".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["depPath"], "/x@1.0.0");
    assert_eq!(json["line"], "hello world");
    assert_eq!(json["stdio"], "stdout");
    assert_eq!(json["stage"], "postinstall");
    assert_eq!(json["wd"], "/wd");
    for k in ["script", "exitCode", "optional"] {
        assert!(json.get(k).is_none(), "Stdio must not carry {k}, got {json:?}");
    }

    eprintln!("CASE: Exit");
    let event = LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Exit {
            dep_path: "/x@1.0.0".to_string(),
            exit_code: 0,
            optional: false,
            stage: "postinstall".to_string(),
            wd: "/wd".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["depPath"], "/x@1.0.0");
    assert_eq!(json["exitCode"], 0);
    assert_eq!(json["optional"], false);
    assert_eq!(json["stage"], "postinstall");
    assert_eq!(json["wd"], "/wd");
    for k in ["script", "line", "stdio"] {
        assert!(json.get(k).is_none(), "Exit must not carry {k}, got {json:?}");
    }
}

/// `pnpm:ignored-scripts` carries a single field: `packageNames` (camelCase).
/// Default-reporter needs the camelCase spelling.
#[test]
fn ignored_scripts_event_matches_pnpm_wire_shape() {
    let event = LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: vec!["foo@1.0.0".to_string(), "bar@2.0.0".to_string()],
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
}

/// `pnpm:skipped-optional-dependency` matches upstream's wire
/// shape: top-level `details`, `package: { id, name, version }`,
/// `prefix`, and `reason` (`snake_case`). Mirrors
/// `SkippedOptionalDependencyMessage` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/core/core-loggers/src/skippedOptionalDependencyLogger.ts>.
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

/// `pnpm:_broken_node_modules` carries a single `missing` field with
/// the absolute path of the slot that should have been on disk but
/// wasn't. Mirrors upstream's emit shape at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L258>.
#[test]
fn broken_modules_event_matches_pnpm_wire_shape() {
    let event = LogEvent::BrokenModules(BrokenModulesLog {
        level: LogLevel::Debug,
        missing: "/proj/node_modules/.pacquet/react@18.0.0/node_modules/react".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:_broken_node_modules");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["missing"], "/proj/node_modules/.pacquet/react@18.0.0/node_modules/react");
}

/// `resolution_failure` payload uses the second upstream variant:
/// no `id`, optional `name` / `version`, and a `bareSpecifier`
/// (camelCase on the wire). Mirrors the `package` shape at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/core/core-loggers/src/skippedOptionalDependencyLogger.ts#L21-L29>:
/// pnpm renders the resolver-time emit with whatever fields the
/// resolver had at fail time — bare specifier always present;
/// `name` / `version` only when the resolver advanced far enough
/// to extract them.
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
        assert_eq!(json, format!("\"{expected}\""), "{reason:?} must serialize as {expected:?}");
    }
}

/// Phase markers serialize as the `snake_case` strings pnpm uses.
#[test]
fn stage_phases_serialize_in_pnpm_form() {
    let cases = [
        (Stage::ResolutionStarted, "resolution_started"),
        (Stage::ResolutionDone, "resolution_done"),
        (Stage::ImportingStarted, "importing_started"),
        (Stage::ImportingDone, "importing_done"),
    ];
    for (stage, expected) in cases {
        let json = serde_json::to_string(&stage).expect("serialize stage");
        assert_eq!(json, format!("\"{expected}\""), "phase {expected}");
    }
}

/// [`SilentReporter`] is observably a no-op. Any test fake is harder
/// to write than just calling it.
#[test]
fn silent_reporter_drops_events() {
    // The point is that no panic, no I/O, and no observable side
    // effect happens. The test passes by virtue of the call returning.
    SilentReporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: String::new(),
        stage: Stage::ImportingStarted,
    }));
}

#[test]
fn recording_fake_captures_emitted_events() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    fn install_step<Reporter: self::Reporter>() {
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: "/proj".to_string(),
            stage: Stage::ImportingStarted,
        }));
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: "/proj".to_string(),
            stage: Stage::ImportingDone,
        }));
    }

    install_step::<RecordingReporter>();

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert!(matches!(
        &captured[0],
        LogEvent::Stage(StageLog { stage: Stage::ImportingStarted, .. })
    ));
    assert!(matches!(&captured[1], LogEvent::Stage(StageLog { stage: Stage::ImportingDone, .. })));
}

/// `pnpm:lockfile-verification` `started` event carries `entries` and
/// the camelCase `lockfilePath`, both flattened into the envelope
/// alongside `status: "started"`. Mirrors upstream's emit at
/// <https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts#L135-L139>.
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

/// `pnpm:lockfile-verification` `done` event adds `elapsedMs` in
/// camelCase, with `status: "done"`. Matches upstream's emit at
/// <https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts#L163-L168>.
#[test]
fn lockfile_verification_done_event_matches_pnpm_wire_shape() {
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Done {
            entries: 12,
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
    assert_eq!(json["elapsedMs"], 999);
    assert_eq!(json["lockfilePath"], "/proj/pnpm-lock.yaml");
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

/// A test fake of [`GetHostName`] returns whatever value its impl
/// declares. This proves the capability trait is dispatchable from a
/// test, which is what consumers of the trait need to know.
#[test]
fn get_host_name_capability_is_mockable() {
    struct FakeHostName;
    impl GetHostName for FakeHostName {
        fn get_host_name() -> String {
            "fixture-host".to_owned()
        }
    }
    assert_eq!(FakeHostName::get_host_name(), "fixture-host");
}

/// [`Host::get_host_name`] returns the value of `gethostname(2)`,
/// which any real environment populates with at least one byte.
#[test]
fn host_returns_a_non_empty_host_name() {
    let host = Host::get_host_name();
    eprintln!("Host::get_host_name() = {host:?}");
    assert!(!host.is_empty());
}
