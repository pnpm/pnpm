use super::{render, state_with_options};
use pnpm_default_reporter::{
    SummaryScope,
    state::{ReporterOptions, ScopeOptions},
};
use pnpm_reporter::{LogEvent, LogLevel, PeerDependencyIssuesLog};

#[test]
fn peer_warning_targets_the_command_scope_and_is_printed_once() {
    for (is_global, summary, command) in [
        (false, SummaryScope::CurrentPrefix, "pnpm peers check"),
        (false, SummaryScope::AllPrefixes, "pnpm peers check"),
        (true, SummaryScope::AllPrefixes, "pnpm peers check -g"),
    ] {
        let mut reporter = state_with_options(ReporterOptions {
            scope: ScopeOptions { is_global, summary, ..Default::default() },
            ..Default::default()
        });
        let event = LogEvent::PeerDependencyIssues(PeerDependencyIssuesLog {
            level: LogLevel::Debug,
            issues_by_projects: serde_json::json!({}),
        });
        let output = render(&mut reporter, vec![event.clone(), event]);
        assert_eq!(
            output,
            format!(r#"[WARN] Issues with peer dependencies found. Run "{command}" to list them."#),
        );
    }
}
