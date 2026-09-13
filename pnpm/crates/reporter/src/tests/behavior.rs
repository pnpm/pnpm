use super::{
    AddedRoot, BrokenModulesLog, ContextLog, DedupeCheckLog, DependencyType, DeprecationLog,
    Envelope, GetHostName, GlobalLog, HookLog, Host, LogEvent, LogLevel, PackageImportMethod,
    PackageImportMethodLog, Pipe, PnpmErrorLog, PnpmLog, PromptAction, PromptLog, RemovedRoot,
    RequestRetryError, RequestRetryLog, RootLog, RootMessage, Stage, StageLog, StatsLog,
    StatsMessage, Value, assert_eq,
};

#[test]
fn prompt_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Prompt(PromptLog { level: LogLevel::Debug, action: PromptAction::Start });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:prompt");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["action"], "start");
}

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

#[test]
fn dedupe_check_event_matches_pnpm_wire_shape() {
    let event = LogEvent::DedupeCheck(DedupeCheckLog {
        level: LogLevel::Error,
        message: "Dedupe --check found changes to the lockfile".to_string(),
        err: PnpmErrorLog {
            code: "ERR_PNPM_DEDUPE_CHECK_ISSUES".to_string(),
            message: "Dedupe --check found changes to the lockfile".to_string(),
        },
        dedupe_check_issues: serde_json::json!({
            "importerIssuesByImporterId": {
                "added": [],
                "removed": [],
                "updated": {},
            },
            "packageIssuesByDepPath": {
                "added": ["dep@2.0.0"],
                "removed": ["dep@1.0.0"],
                "updated": {},
            },
        }),
        rendered: "terminal-only rendering".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm");
    assert_eq!(json["level"], "error");
    assert_eq!(json["err"]["code"], "ERR_PNPM_DEDUPE_CHECK_ISSUES");
    assert_eq!(json["dedupeCheckIssues"]["packageIssuesByDepPath"]["added"][0], "dep@2.0.0");
    assert!(json.get("rendered").is_none(), "terminal rendering must stay off the wire: {json:?}");
}

/// Global-channel (`name: "pnpm:global"`) log carries the
/// `pnpm:global` channel name and a bare `message` with no `prefix` —
/// matching pnpm's `bole('pnpm:global')` writes. A `prefix` field would
/// diverge from the upstream shape `@pnpm/cli.default-reporter` parses.
#[test]
fn global_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: "Authenticate your account at:\nhttps://registry.npmjs.org/auth/abc".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:global");
    assert_eq!(json["level"], "info");
    assert_eq!(
        json["message"],
        "Authenticate your account at:\nhttps://registry.npmjs.org/auth/abc",
    );
    assert!(json.get("prefix").is_none(), "pnpm:global must not carry a prefix, got {json:?}");
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
/// kebab-case `clone-or-copy` that `pnpm_config::PackageImportMethod`
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
        assert_eq!(json, format!(r#""{expected}""#));
    }
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
        assert_eq!(json, format!(r#""{expected}""#));
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

/// `pnpm:_broken_node_modules` carries a single `missing` field with
/// the absolute path of the slot that should have been on disk but
/// wasn't.
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
        assert_eq!(json, format!(r#""{expected}""#), "phase {expected}");
    }
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

/// `pnpm:deprecation` event serializes to the same JSON shape
/// pnpm's `@pnpm/cli.default-reporter` expects.
#[test]
fn deprecation_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Deprecation(DeprecationLog {
        level: LogLevel::Debug,
        pkg_name: "express".to_string(),
        pkg_version: "0.14.1".to_string(),
        pkg_id: "express@0.14.1".to_string(),
        prefix: "/projects/x".to_string(),
        deprecated: "express 0.x series is deprecated".to_string(),
        depth: 0,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["name"], "pnpm:deprecation");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["pkgName"], "express");
    assert_eq!(json["pkgVersion"], "0.14.1");
    assert_eq!(json["pkgId"], "express@0.14.1");
    assert_eq!(json["prefix"], "/projects/x");
    assert_eq!(json["deprecated"], "express 0.x series is deprecated");
    assert_eq!(json["depth"], 0);
}

/// Transitive deprecation events (`depth > 0`) also match the wire shape.
#[test]
fn deprecation_event_transitive_matches_pnpm_wire_shape() {
    let event = LogEvent::Deprecation(DeprecationLog {
        level: LogLevel::Debug,
        pkg_name: "request".to_string(),
        pkg_version: "2.88.2".to_string(),
        pkg_id: "request@2.88.2".to_string(),
        prefix: "/projects/x".to_string(),
        deprecated: "request has been deprecated".to_string(),
        depth: 3,
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:deprecation");
    assert_eq!(json["pkgName"], "request");
    assert_eq!(json["depth"], 3);
}
