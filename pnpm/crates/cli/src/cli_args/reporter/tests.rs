use super::EventFilter;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, PnpmLog, StatsLog, StatsMessage, SummaryLog};

fn summary() -> LogEvent {
    LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix: "/project".to_string() })
}

fn stats() -> LogEvent {
    LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: "/project".to_string(), added: 1 },
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

#[test]
fn no_filter_hides_nothing() {
    for event in [summary(), stats(), pnpm_message("Already up to date"), warning()] {
        assert!(!EventFilter::None.hides(&event), "{event:?}");
    }
}

#[test]
fn resolution_pass_hides_install_tree_events_and_the_up_to_date_message() {
    for event in [summary(), stats(), pnpm_message("Already up to date")] {
        assert!(EventFilter::GlobalUpdateResolution.hides(&event), "{event:?}");
    }
    for event in [pnpm_message("Progress resolved"), warning()] {
        assert!(!EventFilter::GlobalUpdateResolution.hides(&event), "{event:?}");
    }
}

#[test]
fn materialization_pass_hides_only_the_summary() {
    assert!(EventFilter::GlobalUpdateMaterialization.hides(&summary()));
    for event in [stats(), pnpm_message("Already up to date"), warning()] {
        assert!(!EventFilter::GlobalUpdateMaterialization.hides(&event), "{event:?}");
    }
}

#[test]
fn quiet_work_hides_everything() {
    for event in [summary(), stats(), pnpm_message("Already up to date"), warning()] {
        assert!(EventFilter::All.hides(&event), "{event:?}");
    }
}
