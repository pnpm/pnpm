use super::{EnvVar, PmOnFail, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq, fs};
use crate::{
    SettingsRead,
    workspace_yaml::{parse_settings, read_readable_settings},
};
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
    find_and_read_selecting(text, SettingsRead::Readable)
}

fn find_and_read_selecting(text: &str, settings_read: SettingsRead) -> WorkspaceSettings {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), text).unwrap();
    let (_, settings) = WorkspaceSettings::find_and_read(dir.path(), settings_read)
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

#[test]
fn the_pin_decision_reads_only_its_own_settings() {
    let settings = find_and_read_selecting(
        concat!(
            "nodeLinker: hoisted\n",
            "pmOnFail: warn\n",
            "tasks:\n",
            "  build:\n",
            "    concurrency: 0\n",
            "lockfile:\n",
            "  recordsSomethingNew: true\n",
            "frozenLockfile: true\n",
            "minimumReleaseAge: 1440\n",
        ),
        SettingsRead::PinDecision,
    );

    assert_eq!(settings.pm_on_fail, Some(PmOnFail::Warn));
    assert_eq!(settings.frozen_lockfile, Some(true));
    assert_eq!(settings.lockfile, None, "an unreadable pin decision setting is left out");
    assert_eq!(settings.node_linker, None);
    assert_eq!(settings.tasks, None);
    assert_eq!(settings.minimum_release_age, None);
    assert!(settings.key_issues.is_empty(), "{:?}", settings.key_issues);
}

#[test]
fn the_pin_decision_reads_its_settings_from_a_flow_mapping() {
    let settings = find_and_read_selecting(
        "{pmOnFail: warn, nodeLinker: hoisted, lockfile: {recordsSomethingNew: true}, frozenLockfile: true}\n",
        SettingsRead::PinDecision,
    );

    assert_eq!(settings.pm_on_fail, Some(PmOnFail::Warn));
    assert_eq!(settings.frozen_lockfile, Some(true));
    assert_eq!(settings.lockfile, None, "an unreadable pin decision setting is left out");
    assert_eq!(settings.node_linker, None);
}

#[test]
fn the_pin_decision_reads_the_workspace_packages() {
    let settings = find_and_read_selecting(
        "packages:\n  - packages/*\nnodeLinker: hoisted\n",
        SettingsRead::PinDecision,
    );

    assert_eq!(settings.packages, Some(vec!["packages/*".to_string()]));
    assert_eq!(settings.node_linker, None);
}
