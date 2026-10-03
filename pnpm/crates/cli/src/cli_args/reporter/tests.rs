use super::{CliReporter, EventFilter, ReporterType, select_reporter};
use pnpm_reporter::{
    GlobalLog, LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, PnpmLog,
    RemovedRoot, Reporter, RootLog, RootMessage, StatsLog, StatsMessage, SummaryLog,
};

fn summary() -> LogEvent {
    LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix: "/project".to_string() })
}

fn stats() -> LogEvent {
    LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: "/project".to_string(), added: 1 },
    })
}

fn package_manifest() -> LogEvent {
    LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Initial {
            prefix: "/project".to_string(),
            initial: serde_json::json!({}),
        },
    })
}

fn root() -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Removed {
            prefix: "/project".to_string(),
            removed: RemovedRoot { name: "dep".to_string(), version: None, dependency_type: None },
        },
    })
}

fn pnpm_message(message: &str) -> LogEvent {
    LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: message.to_string(),
        prefix: "/project".to_string(),
    })
}

fn warning() -> LogEvent {
    LogEvent::Global(GlobalLog { level: LogLevel::Warn, message: "warning".to_string() })
}

fn install_tree_events() -> [LogEvent; 4] {
    [summary(), stats(), package_manifest(), root()]
}

#[test]
fn no_filter_hides_nothing() {
    for event in install_tree_events()
        .into_iter()
        .chain([pnpm_message("Already up to date"), warning()])
    {
        assert!(!EventFilter::None.hides(&event), "{event:?}");
    }
}

#[test]
fn resolution_pass_hides_install_tree_events_and_the_up_to_date_message() {
    for event in install_tree_events()
        .into_iter()
        .chain([pnpm_message("Already up to date")])
    {
        assert!(EventFilter::GlobalUpdateResolution.hides(&event), "{event:?}");
    }
    for event in [pnpm_message("Progress resolved"), warning()] {
        assert!(!EventFilter::GlobalUpdateResolution.hides(&event), "{event:?}");
    }
}

#[test]
fn materialization_pass_hides_only_the_summary() {
    assert!(EventFilter::GlobalUpdateMaterialization.hides(&summary()));
    for event in
        [stats(), package_manifest(), root(), pnpm_message("Already up to date"), warning()]
    {
        assert!(!EventFilter::GlobalUpdateMaterialization.hides(&event), "{event:?}");
    }
}

#[test]
fn quiet_work_hides_everything() {
    for event in install_tree_events()
        .into_iter()
        .chain([pnpm_message("Already up to date"), warning()])
    {
        assert!(EventFilter::All.hides(&event), "{event:?}");
    }
}

#[test]
fn a_nested_guard_restores_the_outer_filter() {
    let outer = EventFilter::All.apply();
    drop(EventFilter::GlobalUpdateMaterialization.apply());
    assert_eq!(EventFilter::current(), EventFilter::All);
    drop(outer);
    assert_eq!(EventFilter::current(), EventFilter::None);
}

#[test]
fn a_fatal_error_is_returned_for_rendering_only_without_a_filter() {
    select_reporter(ReporterType::Default);
    assert_eq!(CliReporter::report_fatal_error("failed".to_string()).as_deref(), Some("failed"));
    let _quiet = EventFilter::All.apply();
    assert_eq!(CliReporter::report_fatal_error("failed".to_string()), None);
}
