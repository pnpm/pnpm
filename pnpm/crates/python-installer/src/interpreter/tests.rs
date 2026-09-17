use super::{
    InterpreterCommand, Interpreters, VersionRequest,
    request::{parse_version_request, version_request},
};
use pnpm_reporter::SilentReporter;
use std::path::PathBuf;

fn request(line: &str) -> Option<VersionRequest> {
    parse_version_request(line, PathBuf::from(".python-version"))
}

fn version(version: &str) -> pep440_rs::Version {
    version.parse().expect("interpreter version fixture")
}

#[test]
fn a_python_version_file_asks_for_every_release_its_version_is_a_prefix_of() {
    let minor = request("3.13\n").expect("a version request");
    assert!(minor.accepts(&version("3.13.1")));
    assert!(minor.accepts(&version("3.13.0")));
    assert!(!minor.accepts(&version("3.14.0")));
    assert_eq!(minor.version(), "3.13");

    let exact = request("3.13.13").expect("a version request");
    assert!(exact.accepts(&version("3.13.13")));
    assert!(!exact.accepts(&version("3.13.1")));

    let major = request("3").expect("a version request");
    assert!(major.accepts(&version("3.9.1")));
    assert!(!major.accepts(&version("4.0.0")));
}

#[test]
fn a_python_version_file_written_for_another_tool_asks_for_nothing() {
    assert!(
        request("# only a comment\n\n3.13\n").is_some_and(|request| request.version() == "3.13"),
    );
    // pyenv names distributions in this file; those are for the tool that wrote it.
    assert!(request("pypy@3.10\n").is_none());
    assert!(request("miniconda3-4.7.12\n").is_none());
    assert!(request("3.13t\n").is_none());
    assert!(request("\n").is_none());
}

#[test]
fn the_nearest_python_version_file_wins_and_the_search_stops_at_the_workspace() {
    let workspace = tempfile::tempdir().expect("workspace directory");
    let project = workspace.path().join("packages/app");
    std::fs::create_dir_all(&project).expect("project directory");
    std::fs::write(workspace.path().join(".python-version"), "3.12\n").expect("workspace pin");

    let stop = Some(workspace.path());
    let found = version_request::<SilentReporter>(stop, &project).expect("the workspace pin");
    assert_eq!(found.map(|request| request.version()).as_deref(), Some("3.12"));

    std::fs::write(project.join(".python-version"), "3.13\n").expect("project pin");
    let found = version_request::<SilentReporter>(stop, &project).expect("the project pin");
    assert_eq!(found.map(|request| request.version()).as_deref(), Some("3.13"));

    let found =
        version_request::<SilentReporter>(stop, workspace.path()).expect("the workspace pin");
    assert_eq!(found.map(|request| request.version()).as_deref(), Some("3.12"));
}

#[test]
fn a_requested_version_is_tried_by_name_after_the_conventional_ones() {
    let named = Interpreters::named(request("3.13.1").as_ref())
        .iter()
        .map(InterpreterCommand::to_string)
        .collect::<Vec<_>>();
    dbg!(&named);
    let requested = named
        .iter()
        .position(|command| command == if cfg!(windows) { "py -3.13" } else { "python3.13" })
        .expect("the requested version is a candidate");
    let conventional = named
        .iter()
        .position(|command| command == "python3" || command == "python")
        .expect("the conventional names are candidates");
    assert!(conventional < requested, "{named:?}");
    assert_eq!(
        named
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        named.len(),
    );
}
