use super::{
    Output,
    flush_checks,
    render,
    state_with_options,
};
use pnpm_default_reporter::{
    SummaryScope,
    state::{
        ReporterOptions,
        ScopeOptions,
    },
};
use pnpm_reporter::{
    LogEvent,
    LogLevel,
    PeerDependencyIssuesLog,
};

#[test]
fn peer_issues_row_targets_the_command_scope_and_is_printed_once() {
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
        render(&mut reporter, vec![event.clone(), event]);
        let Output::Frame(frame) = flush_checks(&mut reporter) else {
            panic!("the footer renders");
        };
        assert_eq!(frame, format!("\n✗ Peer dependencies have issues (see \"{command}\")\n"));
    }
}

#[test]
fn peer_issues_row_names_the_peers() {
    let mut reporter = state_with_options(ReporterOptions::default());
    let issues = serde_json::json!({
        ".": { "bad": { "react": [] }, "missing": { "@types/react": [] }, "conflicts": [], "intersections": {} },
        "packages/web": { "bad": { "react": [] }, "missing": {}, "conflicts": [], "intersections": {} },
    });
    render(
        &mut reporter,
        vec![LogEvent::PeerDependencyIssues(PeerDependencyIssuesLog {
            level: LogLevel::Debug,
            issues_by_projects: issues,
        })],
    );
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert_eq!(
        frame,
        "\n✗ 2 peer dependencies have issues: @types/react and react (see \"pnpm peers check\")\n",
    );
}

#[test]
fn footer_highlights_package_names_and_commands() {
    let mut reporter = super::state(true);
    render(
        &mut reporter,
        vec![LogEvent::PeerDependencyIssues(PeerDependencyIssuesLog {
            level: LogLevel::Debug,
            issues_by_projects: serde_json::json!({ ".": { "bad": { "react": [] } } }),
        })],
    );
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert_eq!(
        frame,
        "\n\u{1b}[31m✗\u{1b}[39m 1 peer dependency has issues: \u{1b}[35mreact\u{1b}[39m \
         (see \"\u{1b}[36mpnpm peers check\u{1b}[39m\")\n",
    );
}
