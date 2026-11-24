use super::{
    CWD,
    LogEvent,
    LogLevel,
    MaxLogLevel,
    Output,
    ReporterOptions,
    ReporterState,
    SkippedOptionalDependencyLog,
    SkippedOptionalPackage,
    SkippedOptionalReason,
    emitted_lines,
    lifecycle_exit,
    lifecycle_line,
    lifecycle_script,
    render,
    state_with_options,
};
use pnpm_default_reporter::state::{
    CommandOutcome,
    LifecycleOptions,
};
use pnpm_reporter::{
    LifecycleLog,
    LifecycleMessage,
    SideEffectsCacheHitLog,
};

const PROJECT: &str = "/repo/pnpm/npm/pnpm";
const ESBUILD: &str = "/repo/node_modules/.pnpm/esbuild@0.21.5/node_modules/esbuild";

fn summarizing(append_only: bool) -> ReporterState {
    state_with_options(ReporterOptions {
        append_only,
        lifecycle: LifecycleOptions { summarize_scripts: true, ..Default::default() },
        ..Default::default()
    })
}

fn script_run(wd: &str, stage: &str, exit_code: i32) -> Vec<LogEvent> {
    vec![lifecycle_script(wd, stage, "node install.js"), lifecycle_exit(wd, stage, exit_code)]
}

fn dependency_script(dep_path: &str) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Script {
            dep_path: dep_path.to_string(),
            optional: false,
            script: "node install.js".to_string(),
            stage: "postinstall".to_string(),
            wd: ESBUILD.to_string(),
        },
    })
}

fn dependency_exit(dep_path: &str, exit_code: i32, optional: bool) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Exit {
            dep_path: dep_path.to_string(),
            exit_code,
            optional,
            stage: "postinstall".to_string(),
            wd: ESBUILD.to_string(),
        },
    })
}

fn cache_hit(name: &str, version: &str) -> LogEvent {
    LogEvent::SideEffectsCacheHit(SideEffectsCacheHitLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        dep_path: format!("{name}@{version}"),
        pkg_name: name.to_string(),
        pkg_version: version.to_string(),
    })
}

fn failed_optional_build(name: &str, version: &str) -> LogEvent {
    LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some("exit code 1".to_string()),
        package: SkippedOptionalPackage::Installed {
            id: format!("/repo/node_modules/{name}"),
            name: name.to_string(),
            version: version.to_string(),
        },
        parents: None,
        prefix: CWD.to_string(),
        reason: SkippedOptionalReason::BuildFailure,
    })
}

fn footer(reporter: &mut ReporterState, outcome: CommandOutcome) -> String {
    match reporter.flush_pending_checks(outcome) {
        Output::Frame(frame) => frame,
        Output::Lines(lines) => lines.join("\n"),
        Output::None => String::new(),
    }
}

#[test]
fn a_running_script_shows_until_it_succeeds() {
    let mut reporter = summarizing(false);
    let running =
        render(&mut reporter, vec![lifecycle_script(PROJECT, "preinstall", "node install.js")]);
    assert!(running.contains("Running..."), "{running}");
    let done = render(&mut reporter, vec![lifecycle_exit(PROJECT, "preinstall", 0)]);
    assert_eq!(done, "");
}

#[test]
fn footer_lists_the_project_scripts_that_ran() {
    let mut reporter = summarizing(false);
    let mut events = script_run(PROJECT, "preinstall", 0);
    events.extend(script_run(PROJECT, "postinstall", 0));
    events.extend(script_run(CWD, "prepare", 0));
    render(&mut reporter, events);
    assert_eq!(
        footer(&mut reporter, CommandOutcome::Succeeded),
        "\n✓ 3 project scripts ran: . prepare, pnpm/npm/pnpm postinstall and pnpm/npm/pnpm preinstall\n",
    );
}

#[test]
fn footer_lists_built_and_cached_dependencies() {
    let mut reporter = summarizing(false);
    let frame = render(
        &mut reporter,
        vec![
            dependency_script("esbuild@0.21.5(typescript@5.4.5)"),
            dependency_exit("esbuild@0.21.5(typescript@5.4.5)", 0, false),
            cache_hit("sharp", "0.33.5"),
        ],
    );
    assert_eq!(frame, "");
    assert_eq!(
        footer(&mut reporter, CommandOutcome::Succeeded),
        "\n✓ 1 dependency was built: esbuild@0.21.5\n\
         ✓ 1 dependency build came from the side-effects cache: sharp@0.33.5\n",
    );
}

#[test]
fn a_failed_optional_build_is_erased_and_listed_as_a_problem() {
    let mut reporter = summarizing(false);
    let frame = render(
        &mut reporter,
        vec![
            dependency_script("esbuild@0.21.5"),
            dependency_exit("esbuild@0.21.5", 1, true),
            failed_optional_build("esbuild", "0.21.5"),
        ],
    );
    assert_eq!(frame, "");
    assert_eq!(
        footer(&mut reporter, CommandOutcome::Failed),
        "\n✗ 1 optional dependency failed to build and was skipped: esbuild@0.21.5\n",
    );
}

#[test]
fn a_failed_script_keeps_its_output() {
    let mut reporter = summarizing(false);
    let frame = render(
        &mut reporter,
        vec![
            lifecycle_script(PROJECT, "preinstall", "node install.js"),
            lifecycle_line(PROJECT, "preinstall", "boom"),
            lifecycle_exit(PROJECT, "preinstall", 1),
        ],
    );
    assert!(frame.contains("boom") && frame.contains("Failed in"), "{frame}");
    assert!(matches!(reporter.flush_pending_checks(CommandOutcome::Failed), Output::None));
}

#[test]
fn a_failed_command_prints_no_script_successes() {
    let mut reporter = summarizing(false);
    render(&mut reporter, script_run(PROJECT, "preinstall", 0));
    assert!(matches!(reporter.flush_pending_checks(CommandOutcome::Failed), Output::None));
}

#[test]
fn append_only_prints_only_script_output_and_failures() {
    let mut reporter = summarizing(true);
    let mut events = script_run(CWD, "prepare", 0);
    events.extend([
        lifecycle_script(PROJECT, "preinstall", "node install.js"),
        lifecycle_line(PROJECT, "preinstall", "fetched the binary"),
        lifecycle_exit(PROJECT, "preinstall", 0),
    ]);
    events.extend(script_run(PROJECT, "postinstall", 1));
    assert_eq!(
        emitted_lines(&mut reporter, events).join("\n"),
        "pnpm/npm/pnpm preinstall$ node install.js\n\
         pnpm/npm/pnpm preinstall: fetched the binary\n\
         pnpm/npm/pnpm postinstall$ node install.js\n\
         pnpm/npm/pnpm postinstall: Failed",
    );
}

#[test]
fn quiet_output_hides_successful_script_output() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Warn,
        lifecycle: LifecycleOptions { summarize_scripts: true, ..Default::default() },
        ..Default::default()
    });
    let events = vec![
        lifecycle_script(PROJECT, "preinstall", "node install.js"),
        lifecycle_line(PROJECT, "preinstall", "fetched the binary"),
        lifecycle_exit(PROJECT, "preinstall", 0),
    ];
    assert_eq!(render(&mut reporter, events), "");
}
