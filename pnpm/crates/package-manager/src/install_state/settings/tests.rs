use super::{
    reference_time_moved_back,
    settings_match,
};
use pnpm_workspace_state::WorkspaceStateSettings;
use std::collections::BTreeMap;

#[test]
fn reference_time_drifts_only_when_it_moves_back() {
    let at = |value: &str| pnpm_time::parse_rfc3339(value).expect("valid");
    let now = at("2026-03-01T00:00:00Z");
    let early = Some("2026-01-01T00:00:00Z");
    let late = Some("2026-02-01T00:00:00Z");
    let ahead = Some("2026-03-01T00:05:00Z");
    assert!(!reference_time_moved_back(early, late, now), "moved forward");
    assert!(!reference_time_moved_back(early, early, now), "unchanged");
    assert!(!reference_time_moved_back(early, None, now), "removed while in the past");
    assert!(!reference_time_moved_back(None, None, now), "never set");
    assert!(reference_time_moved_back(late, early, now), "moved back");
    assert!(reference_time_moved_back(None, early, now), "set where the record used its own clock");
    assert!(reference_time_moved_back(ahead, None, now), "removed while ahead of the clock");
}

#[test]
fn recorded_settings_are_compared_with_the_gate_ignoring_its_keys() {
    let current = WorkspaceStateSettings { dev: Some(true), ..WorkspaceStateSettings::default() };
    let recorded = serde_json::to_string(&WorkspaceStateSettings {
        dev: Some(false),
        ..WorkspaceStateSettings::default()
    })
    .expect("serialize recorded settings");

    assert!(settings_match(&recorded, &current, &["dev"], None).expect("parse recorded settings"));
    assert!(!settings_match(&recorded, &current, &[], None).expect("parse recorded settings"));
}

#[test]
fn recorded_installed_registry_routes_match_current_routes() {
    let routes =
        BTreeMap::from([("default".to_string(), "https://registry.example.test".to_string())]);
    let recorded = serde_json::to_string(&WorkspaceStateSettings {
        installed_registry_routes: Some(routes.clone()),
        ..WorkspaceStateSettings::default()
    })
    .expect("serialize recorded settings");
    let current = WorkspaceStateSettings {
        registry_routes: Some(routes),
        ..WorkspaceStateSettings::default()
    };

    assert!(settings_match(&recorded, &current, &[], None).expect("parse recorded settings"));
}

#[test]
fn recorded_lockfile_routes_still_differ_from_installed_routes() {
    let installed =
        BTreeMap::from([("@scope".to_string(), "https://installed.example.test".to_string())]);
    let lockfile =
        BTreeMap::from([("@scope".to_string(), "https://lockfile.example.test".to_string())]);
    let recorded = serde_json::to_string(&WorkspaceStateSettings {
        installed_registry_routes: Some(installed.clone()),
        registry_routes: Some(lockfile),
        ..WorkspaceStateSettings::default()
    })
    .expect("serialize recorded settings");
    let current = WorkspaceStateSettings {
        registry_routes: Some(installed),
        ..WorkspaceStateSettings::default()
    };

    assert!(!settings_match(&recorded, &current, &[], None).expect("parse recorded settings"));
}

#[test]
fn malformed_recorded_settings_do_not_panic() {
    assert!(settings_match("{", &WorkspaceStateSettings::default(), &[], None).is_err());
}
