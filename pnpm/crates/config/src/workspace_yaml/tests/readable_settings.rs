use super::{PmOnFail, assert_eq};
use crate::workspace_yaml::readable::parse_readable_settings;
use pnpm_env_replace::SystemEnv;

#[test]
fn a_setting_in_a_newer_shape_is_left_out() {
    let settings = parse_readable_settings::<SystemEnv>(concat!(
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
    let settings = parse_readable_settings::<SystemEnv>(concat!(
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
    let settings = parse_readable_settings::<SystemEnv>("ignoreScripts: true\n").unwrap();

    assert_eq!(settings.ignore_scripts, Some(true));
}

#[test]
fn a_document_that_is_not_yaml_still_fails() {
    let error = parse_readable_settings::<SystemEnv>("pmOnFail: warn\nlockfile: [\n").unwrap_err();

    eprintln!("{error}");
    assert!(error.location().is_some());
}

#[test]
fn a_flow_mapping_still_fails() {
    parse_readable_settings::<SystemEnv>("{pmOnFail: warn, nodeLinker: [hoisted]}\n").unwrap_err();
}
