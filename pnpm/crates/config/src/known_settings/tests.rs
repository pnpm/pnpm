use super::{
    CONFIG_ONLY_SETTING_KEYS, SETTINGS_OF_OTHER_PNPM_VERSIONS, TYPED_WORKSPACE_MANIFEST_KEYS,
    UNTYPED_WORKSPACE_SETTING_KEYS, annotate_unknown_setting, is_known_setting_key,
    settings_field_keys,
};
use crate::naming_cases::to_camel_case;
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;

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
    assert!(is_known_setting_key("autoInstallPeersFromHighestMatch"));
    assert!(is_known_setting_key("externalDependencies"));
    assert!(is_known_setting_key("modulesDirsByProjectName"));
    assert!(is_known_setting_key("tasks"));
}

/// `globalShims` has no entry in the mirrored pnpm lists; it must enter
/// through the [`crate::WorkspaceSettings`] field names.
#[test]
fn recognizes_pacquet_only_settings_via_the_struct_fields() {
    assert!(is_known_setting_key("globalShims"));
    assert!(is_known_setting_key("cargo"));
    assert!(is_known_setting_key("tools"));
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

fn read_reader_source(file: &str) -> String {
    let path = format!("{}/../../../pnpm11/config/reader/src/{file}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {path}: {err}"))
}

fn block_between<'a>(source: &'a str, header: &str, footer: &str) -> &'a str {
    source
        .split_once(header)
        .and_then(|(_, rest)| rest.split_once(footer))
        .map_or_else(|| panic!("missing block `{header}`"), |(body, _)| body)
}

fn quoted_array_items<'a>(source: &'a str, declaration: &str) -> Vec<&'a str> {
    block_between(source, declaration, "]")
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('\'')?
                .strip_suffix("',")
        })
        .collect()
}

fn ts_object_keys<'a>(source: &'a str, declaration: &str) -> Vec<&'a str> {
    block_between(source, declaration, "\n}")
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('*') {
                return None;
            }
            let (key, _) = trimmed.split_once(':')?;
            Some(key.trim().trim_matches('\''))
        })
        .collect()
}

#[test]
fn mirrored_lists_and_workspace_settings_match_unknown_settings_ts() {
    let unknown_settings_ts = read_reader_source("unknownSettings.ts");
    let types_ts = read_reader_source("types.ts");
    let npm_config_types_ts = read_reader_source("npmConfigTypes.ts");

    let ts_typed =
        quoted_array_items(&unknown_settings_ts, "const TYPED_WORKSPACE_MANIFEST_KEYS = [");
    let ts_config_only =
        quoted_array_items(&unknown_settings_ts, "const CONFIG_ONLY_SETTING_KEYS = [");
    let ts_untyped =
        quoted_array_items(&unknown_settings_ts, "const UNTYPED_WORKSPACE_SETTING_KEYS = [");

    assert_eq!(TYPED_WORKSPACE_MANIFEST_KEYS, ts_typed.as_slice());
    assert_eq!(CONFIG_ONLY_SETTING_KEYS, ts_config_only.as_slice());

    let mut rust_untyped_and_v11: Vec<&str> = UNTYPED_WORKSPACE_SETTING_KEYS
        .iter()
        .copied()
        .chain(SETTINGS_OF_OTHER_PNPM_VERSIONS.iter().map(|(key, _)| *key))
        .collect();
    rust_untyped_and_v11.sort_unstable();
    assert_eq!(rust_untyped_and_v11, ts_untyped);

    let v11_known_keys: BTreeSet<String> = ts_typed
        .into_iter()
        .chain(ts_config_only)
        .chain(ts_untyped)
        .map(str::to_string)
        .chain(
            ts_object_keys(&types_ts, "export const pnpmTypes = {")
                .into_iter()
                .chain(ts_object_keys(&npm_config_types_ts, "export const npmConfigTypes = {"))
                .map(to_camel_case),
        )
        .collect();

    let v11_other_versions: BTreeSet<String> = ts_object_keys(
        &unknown_settings_ts,
        "const SETTINGS_OF_OTHER_PNPM_VERSIONS: Record<string, string> = {",
    )
    .into_iter()
    .map(str::to_string)
    .collect();

    let overlap: Vec<&String> = v11_known_keys.intersection(&v11_other_versions).collect();
    assert_eq!(
        overlap,
        Vec::<&String>::new(),
        "keys in both KNOWN_SETTING_KEYS and SETTINGS_OF_OTHER_PNPM_VERSIONS",
    );

    for key in &v11_other_versions {
        assert!(
            is_known_setting_key(key),
            "{key} is listed in unknownSettings.ts SETTINGS_OF_OTHER_PNPM_VERSIONS but is not known to v12",
        );
    }

    let unclassified: Vec<&String> = settings_field_keys()
        .iter()
        .filter(|key| key.as_str() != "_auth")
        .filter(|key| !v11_known_keys.contains(*key) && !v11_other_versions.contains(*key))
        .collect();
    assert_eq!(
        unclassified,
        Vec::<&String>::new(),
        "WorkspaceSettings fields missing from both KNOWN_SETTING_KEYS and SETTINGS_OF_OTHER_PNPM_VERSIONS in unknownSettings.ts",
    );
}
