use super::{EnvVar, PmOnFail, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq, fs};
use crate::workspace_yaml::{parse_settings, read_readable_settings};
use pnpm_env_replace::SystemEnv;

struct Env;

impl EnvVar for Env {
    fn var(name: &str) -> Option<String> {
        (name == "PNPM_TEST_LINKER").then(|| "not-a-linker".to_owned())
    }
}

fn read<Sys: EnvVar>(text: &str) -> Result<WorkspaceSettings, Box<serde_saphyr::Error>> {
    read_readable_settings(text, parse_settings::<Sys>)
}

/// [`WorkspaceSettings::find_and_read`] over a file holding `text`, which
/// validates as well as parses.
fn find_and_read(text: &str) -> WorkspaceSettings {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), text).unwrap();
    let (_, settings) = WorkspaceSettings::find_and_read(dir.path(), true)
        .expect("read pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");
    settings
}

#[test]
fn a_setting_in_a_newer_shape_is_left_out() {
    let settings = read::<SystemEnv>(concat!(
        "pmOnFail: ignore\n",
        "lockfile:\n",
        "  includeResolutionSettings: true\n",
        "  recordsSomethingNew: true\n",
        "# a comment\n",
        "ignoreScripts: true\n",
    ))
    .unwrap();

    assert_eq!(settings.lockfile, None);
    assert_eq!(settings.pm_on_fail, Some(PmOnFail::Ignore));
    assert_eq!(settings.ignore_scripts, Some(true));
}

#[test]
fn every_unreadable_setting_is_left_out() {
    let settings = read::<SystemEnv>(concat!(
        "nodeLinker: [hoisted]\n",
        "pmOnFail: warn\n",
        "ignoreScripts:\n",
        "  inShape: object\n",
    ))
    .unwrap();

    assert_eq!(settings.node_linker, None);
    assert_eq!(settings.ignore_scripts, None);
    assert_eq!(settings.pm_on_fail, Some(PmOnFail::Warn));
}

#[test]
fn a_readable_document_is_read_whole() {
    let settings = read::<SystemEnv>("ignoreScripts: true\n").unwrap();

    assert_eq!(settings.ignore_scripts, Some(true));
}

#[test]
fn a_setting_that_fails_validation_is_left_out() {
    let settings = find_and_read(concat!(
        "pmOnFail: warn\n",
        "tasks:\n",
        "  build:\n",
        "    concurrency: 0\n",
        "ignoreScripts: true\n",
    ));

    assert_eq!(settings.tasks, None);
    assert_eq!(settings.pm_on_fail, Some(PmOnFail::Warn));
    assert_eq!(settings.ignore_scripts, Some(true));
}

#[test]
fn a_document_that_is_not_yaml_still_fails() {
    let error = read::<SystemEnv>("pmOnFail: warn\nlockfile: [\n").unwrap_err();

    eprintln!("{error}");
    assert!(error.location().is_some());
}

#[test]
fn a_flow_mapping_still_fails() {
    read::<SystemEnv>("{pmOnFail: warn, nodeLinker: [hoisted]}\n").unwrap_err();
}

#[test]
fn a_document_that_is_not_a_mapping_still_fails() {
    read::<SystemEnv>("- pmOnFail\n").unwrap_err();
}

#[test]
fn an_invalid_environment_value_is_left_out() {
    let settings = read::<Env>("pmOnFail: warn\nnodeLinker: ${PNPM_TEST_LINKER}\n").unwrap();

    assert_eq!(settings.node_linker, None);
    assert_eq!(settings.pm_on_fail, Some(PmOnFail::Warn));
}
