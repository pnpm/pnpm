use super::{annotate_unknown_setting, is_known_setting_key};

#[test]
fn recognizes_type_keys_in_both_spellings() {
    assert!(is_known_setting_key("minimumReleaseAge"));
    assert!(is_known_setting_key("minimum-release-age"));
    assert!(is_known_setting_key("nodeLinker"));
}

#[test]
fn recognizes_structured_and_config_only_keys() {
    assert!(is_known_setting_key("packages"));
    assert!(is_known_setting_key("catalogs"));
    assert!(is_known_setting_key("overrides"));
    assert!(is_known_setting_key("catalogPrune"));
    assert!(is_known_setting_key("executionEnv"));
}

/// `globalShims` has no entry in the mirrored pnpm lists; it must enter
/// through the [`crate::WorkspaceSettings`] field names.
#[test]
fn recognizes_pacquet_only_settings_via_the_struct_fields() {
    assert!(is_known_setting_key("globalShims"));
}

#[test]
fn rejects_typos_and_inventions() {
    assert!(!is_known_setting_key("minimumReleaseAg"));
    assert!(!is_known_setting_key("zzzNotASettingZzz"));
}

#[test]
fn annotates_a_typo_with_the_closest_setting() {
    assert_eq!(
        annotate_unknown_setting("minimumReleaseAg"),
        r#""minimumReleaseAg" (did you mean "minimumReleaseAge"?)"#,
    );
}

#[test]
fn annotates_a_setting_from_another_pnpm_version() {
    assert_eq!(
        annotate_unknown_setting("confirmModulesPurge"),
        r#""confirmModulesPurge" (a pnpm v11 setting)"#,
    );
}

#[test]
fn annotates_an_unmatchable_key_bare() {
    assert_eq!(annotate_unknown_setting("zzzXqjWv"), r#""zzzXqjWv""#);
}
