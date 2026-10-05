use crate::{
    Lockfile, LockfileSettings, ResolutionSetting, ResolutionSettingDifference, ResolutionSettings,
};
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::path::Path;

#[test]
fn settings_without_the_keys_parse_as_unrecorded() {
    let settings: LockfileSettings =
        serde_saphyr::from_str("autoInstallPeers: true\nexcludeLinksFromLockfile: false\n")
            .unwrap();
    assert_eq!(settings.resolution, ResolutionSettings::default());
}

#[test]
fn recorded_keys_parse_and_save_in_alphabetical_order() {
    let yaml = "\
lockfileVersion: '9.0'

settings:
  autoDedupe: true
  autoInstallPeers: true
  dedupeInjectedDeps: false
  dedupePeerDependents: true
  excludeLinksFromLockfile: false
  linkWorkspacePackages: deep

importers:

  .: {}
";
    let lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    assert_eq!(
        lockfile.settings.as_ref().unwrap().resolution,
        ResolutionSettings {
            auto_dedupe: Some(true),
            dedupe_injected_deps: Some(false),
            dedupe_peer_dependents: Some(true),
            link_workspace_packages: Some(Value::String("deep".to_string())),
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pnpm-lock.yaml");
    lockfile.save_to_path(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), yaml);
}

#[test]
fn first_difference_names_the_first_differing_setting() {
    let expected = ResolutionSettings {
        auto_dedupe: Some(true),
        dedupe_injected_deps: Some(true),
        dedupe_peer_dependents: Some(true),
        link_workspace_packages: Some(Value::Bool(false)),
    };
    assert_eq!(expected.first_difference(&expected), None);
    let recorded = ResolutionSettings { dedupe_peer_dependents: Some(false), ..expected.clone() };
    assert_eq!(
        recorded.first_difference(&expected),
        Some(ResolutionSettingDifference {
            setting: ResolutionSetting::DedupePeerDependents,
            recorded: Some(Value::Bool(false)),
            expected: Some(Value::Bool(true)),
        }),
    );
    assert_eq!(
        ResolutionSettings::default()
            .first_difference(&expected)
            .map(|difference| difference.setting),
        Some(ResolutionSetting::AutoDedupe),
    );
}
