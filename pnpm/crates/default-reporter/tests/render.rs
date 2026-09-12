//! Frame-level tests: drive sequences of `LogEvent`s through `ReporterState`
//! and assert the rendered output, matching what `@pnpm/cli.default-reporter`
//! produces for the same events. Colors are constructed off for readable
//! plain-text assertions and on for the ANSI-specific ones.

use pnpm_default_reporter::{
    MaxLogLevel, SummaryScope,
    colors::Colors,
    format::pretty_bytes,
    state::{Output, ReporterOptions, ReporterState},
};
use pnpm_reporter::{
    AddedRoot, ContextLog, DedupeCheckLog, DependencyType, DeprecationLog, ExecutionTimeLog,
    FetchingProgressLog, FetchingProgressMessage, GlobalLog, HookLog, IgnoredScriptsLog,
    LifecycleLog, LifecycleMessage, LifecycleStdio, LockfileVerificationLog,
    LockfileVerificationMessage, LogEvent, LogLevel, PackageImportMethod, PackageImportMethodLog,
    PackageManifestLog, PackageManifestMessage, PnpmErrorLog, PnpmLog, ProgressLog,
    ProgressMessage, RootLog, RootMessage, ScopeLog, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalParent, SkippedOptionalReason, Stage, StageLog,
    StatsLog, StatsMessage, SummaryLog, UpdateCheckLog,
};

const CWD: &str = "/repo";

fn state(colors: bool) -> ReporterState {
    ReporterState::new(CWD.to_string(), 80, Colors { enabled: colors }, false)
}

fn state_with_options(options: ReporterOptions) -> ReporterState {
    ReporterState::new_with_options(CWD.to_string(), 80, Colors { enabled: false }, options)
}

fn state_without_summary_prefix_filter() -> ReporterState {
    ReporterState::new_with_summary_scope(
        CWD.to_string(),
        80,
        Colors { enabled: false },
        false,
        SummaryScope::AllPrefixes,
    )
}

/// Feed events through the in-place renderer and return the last full frame.
fn render(state: &mut ReporterState, events: Vec<LogEvent>) -> String {
    let mut last = String::new();
    for event in events {
        if let Output::Frame(frame) = state.handle(&event) {
            last = frame;
        }
    }
    last
}

fn progress(status: &str) -> LogEvent {
    progress_at(CWD, status)
}

fn progress_at(requester: &str, status: &str) -> LogEvent {
    let requester = requester.to_string();
    let package_id = "registry.npmjs.org/foo/1.0.0".to_string();
    let message = match status {
        "resolved" => ProgressMessage::Resolved { package_id, requester },
        "fetched" => ProgressMessage::Fetched { package_id, requester },
        "found_in_store" => ProgressMessage::FoundInStore { package_id, requester },
        "imported" => ProgressMessage::Imported {
            method: PackageImportMethod::Hardlink,
            requester,
            to: "/repo/node_modules/foo".to_string(),
        },
        other => panic!("unknown status {other}"),
    };
    LogEvent::Progress(ProgressLog { level: LogLevel::Debug, message })
}

fn stage_at(prefix: &str, stage: Stage) -> LogEvent {
    LogEvent::Stage(StageLog { level: LogLevel::Debug, prefix: prefix.to_string(), stage })
}

fn fetching_started(package_id: &str, size: u64, attempt: u32) -> LogEvent {
    LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt,
            package_id: package_id.to_string(),
            size: Some(size),
        },
    })
}

fn fetching_in_progress(package_id: &str, downloaded: u64) -> LogEvent {
    LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::InProgress {
            downloaded,
            package_id: package_id.to_string(),
        },
    })
}

fn importing_done() -> LogEvent {
    LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        stage: Stage::ImportingDone,
    })
}

fn added_root(name: &str, version: &str, dt: DependencyType) -> LogEvent {
    added_root_at(CWD, name, version, dt)
}

fn added_root_at(prefix: &str, name: &str, version: &str, dt: DependencyType) -> LogEvent {
    added_root_with_latest_at(prefix, name, version, None, dt)
}

fn added_root_with_latest_at(
    prefix: &str,
    name: &str,
    version: &str,
    latest: Option<&str>,
    dt: DependencyType,
) -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: prefix.to_string(),
            added: AddedRoot {
                name: name.to_string(),
                real_name: name.to_string(),
                version: Some(version.to_string()),
                dependency_type: Some(dt),
                id: None,
                latest: latest.map(str::to_string),
                linked_from: None,
            },
        },
    })
}

fn package_manifest_initial_at(prefix: &str, value: serde_json::Value) -> LogEvent {
    LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Initial { prefix: prefix.to_string(), initial: value },
    })
}

fn package_manifest_updated_at(prefix: &str, value: serde_json::Value) -> LogEvent {
    LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix: prefix.to_string(), updated: value },
    })
}

fn summary() -> LogEvent {
    summary_at(CWD)
}

fn summary_at(prefix: &str) -> LogEvent {
    LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix: prefix.to_string() })
}

fn pnpm_log(level: LogLevel, message: &str) -> LogEvent {
    LogEvent::Pnpm(PnpmLog { level, message: message.to_string(), prefix: CWD.to_string() })
}

fn deprecation(name: &str, version: &str, depth: i32, prefix: &str) -> LogEvent {
    LogEvent::Deprecation(DeprecationLog {
        level: LogLevel::Debug,
        pkg_name: name.to_string(),
        pkg_version: version.to_string(),
        pkg_id: format!("{name}@{version}"),
        prefix: prefix.to_string(),
        deprecated: "no longer supported".to_string(),
        depth,
    })
}

fn resolution_done() -> LogEvent {
    LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        stage: Stage::ResolutionDone,
    })
}

fn scope(selected: usize, total: Option<usize>, workspace_prefix: Option<&str>) -> LogEvent {
    LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected,
        total,
        workspace_prefix: workspace_prefix.map(ToString::to_string),
    })
}

fn scope_reporting_state() -> ReporterState {
    state_with_options(ReporterOptions { reports_scope: true, ..ReporterOptions::default() })
}

// --- embedder reporting options ---------------------------------------

fn ignored_scripts(names: &[&str]) -> LogEvent {
    LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Info,
        package_names: names.iter().map(|name| (*name).to_string()).collect(),
        strict_dep_builds: false,
    })
}

fn linked_root(name: &str, from: &str) -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: CWD.to_string(),
            added: AddedRoot {
                name: name.to_string(),
                real_name: name.to_string(),
                version: None,
                dependency_type: Some(DependencyType::Prod),
                id: None,
                latest: None,
                linked_from: Some(from.to_string()),
            },
        },
    })
}

fn update_check(current_version: &str, latest_version: &str) -> LogEvent {
    LogEvent::UpdateCheck(UpdateCheckLog {
        level: LogLevel::Debug,
        current_version: current_version.to_string(),
        latest_version: latest_version.to_string(),
    })
}

fn lifecycle_stdio_events() -> Vec<LogEvent> {
    vec![
        LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Script {
                dep_path: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
                optional: false,
                script: "node install.js".to_string(),
                stage: "postinstall".to_string(),
                wd: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
            },
        }),
        LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Stdio {
                dep_path: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
                line: "downloading the binary".to_string(),
                stage: "postinstall".to_string(),
                stdio: LifecycleStdio::Stdout,
                wd: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
            },
        }),
    ]
}

fn lifecycle_script(wd: &str, stage: &str, script: &str) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Script {
            dep_path: wd.to_string(),
            optional: false,
            script: script.to_string(),
            stage: stage.to_string(),
            wd: wd.to_string(),
        },
    })
}

fn lifecycle_line(wd: &str, stage: &str, line: &str) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Stdio {
            dep_path: wd.to_string(),
            line: line.to_string(),
            stage: stage.to_string(),
            stdio: LifecycleStdio::Stdout,
            wd: wd.to_string(),
        },
    })
}

fn lifecycle_exit(wd: &str, stage: &str, exit_code: i32) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Exit {
            dep_path: wd.to_string(),
            exit_code,
            optional: false,
            stage: stage.to_string(),
            wd: wd.to_string(),
        },
    })
}

/// Two projects whose `postinstall` output interleaves, plus a third
/// that starts and finishes in between.
fn interleaved_lifecycle_events() -> Vec<LogEvent> {
    vec![
        lifecycle_script("/repo/packages/foo", "postinstall", "node foo"),
        lifecycle_line("/repo/packages/foo", "postinstall", "foo I"),
        lifecycle_script("/repo/packages/bar", "postinstall", "node bar"),
        lifecycle_line("/repo/packages/bar", "postinstall", "bar I"),
        lifecycle_line("/repo/packages/foo", "postinstall", "foo II"),
        lifecycle_exit("/repo/packages/bar", "postinstall", 0),
        lifecycle_exit("/repo/packages/foo", "postinstall", 0),
    ]
}

fn emitted_lines(reporter: &mut ReporterState, events: Vec<LogEvent>) -> Vec<String> {
    let mut lines = Vec::new();
    for event in events {
        if let Output::Lines(emitted) = reporter.handle(&event) {
            lines.extend(emitted);
        }
    }
    lines
}

#[path = "render/behavior.rs"]
mod behavior;

#[path = "render/reporting.rs"]
mod reporting;

#[path = "render/security.rs"]
mod security;

#[path = "render/streaming.rs"]
mod streaming;

#[path = "render/dependencies.rs"]
mod dependencies;

#[path = "render/manifests.rs"]
mod manifests;

#[path = "render/lockfile.rs"]
mod lockfile;

#[path = "render/workspace_settings.rs"]
mod workspace_settings;

#[path = "render/files.rs"]
mod files;
