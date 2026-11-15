use super::{is_config_file_key, is_ini_config_key, is_type_key, type_includes_number};

#[test]
fn type_membership() {
    assert!(is_type_key("fetch-retries"));
    assert!(is_type_key("store-dir"));
    assert!(is_type_key("registry"));
    assert!(is_type_key("virtual-store-dir"));
    // Both spellings of the virtual store's type are introspectable, or
    // `pnpm config get` would answer one of them with null.
    assert!(is_type_key("virtual-store-type"));
    assert!(is_type_key("enable-global-virtual-store"));
    assert!(is_type_key("progress"));
    assert!(!is_type_key("macos-backup"));
    assert!(!is_type_key("no-such-setting"));
    // prototype-chain names must not be members
    assert!(!is_type_key("constructor"));
    assert!(!is_type_key("__proto__"));
}

#[test]
fn numeric_types() {
    assert!(type_includes_number("fetch-retries"));
    assert!(type_includes_number("fetch-timeout"));
    assert!(type_includes_number("modules-cache-max-age"));
    assert!(type_includes_number("dlx-cache-max-age"));
    assert!(type_includes_number("umask"));
    assert!(!type_includes_number("store-dir"));
    assert!(!type_includes_number("registry"));
    assert!(!type_includes_number("minimum-release-age-strict"));
}

#[test]
fn ini_keys() {
    assert!(is_ini_config_key("registry"));
    assert!(is_ini_config_key("cafile"));
    assert!(is_ini_config_key("_authToken"));
    assert!(is_ini_config_key("username"));
    assert!(is_ini_config_key("@scope:registry"));
    assert!(is_ini_config_key("//registry.example.com/:_auth"));
    // not auth/scoped/registry keys
    assert!(!is_ini_config_key("store-dir"));
    assert!(!is_ini_config_key("fetch-retries"));
    assert!(!is_ini_config_key("https-proxy"));
    assert!(!is_ini_config_key("no-proxy"));
}

#[test]
fn config_file_keys() {
    // pnpm config-file keys
    assert!(is_config_file_key("store-dir"));
    assert!(is_config_file_key("fetch-timeout"));
    assert!(is_config_file_key("cache-dir"));
    assert!(is_config_file_key("node-download-mirrors"));
    assert!(is_config_file_key("virtual-store-type"));
    assert!(is_config_file_key("enable-global-virtual-store"));
    assert!(is_config_file_key("macos-backup"));
    // A machine-level preference, so it is settable in the global file.
    assert!(is_config_file_key("progress"));
    // npm-compatible, not excluded
    assert!(is_config_file_key("fetch-retries"));
    assert!(is_config_file_key("registry"));
    // structured YAML settings without scalar CLI types
    assert!(is_config_file_key("registries"));
    assert!(is_config_file_key("named-registries"));
    // excluded workspace-only / CLI keys
    assert!(!is_config_file_key("catalog-mode"));
    assert!(!is_config_file_key("node-linker"));
    assert!(!is_config_file_key("hoist"));
    assert!(!is_config_file_key("lockfile"));
    // catalog / package-extensions are not even type keys → not config-file keys
    assert!(!is_config_file_key("catalog"));
    assert!(!is_config_file_key("package-extensions"));
}

#[test]
fn publish_wait_timeout_is_numeric_and_routes_to_yaml() {
    assert!(super::type_includes_number("publish-wait-timeout"));
    assert!(super::is_config_file_key("publish-wait-timeout"));
    assert!(!super::is_ini_config_key("publish-wait-timeout"));
}

/// Mirrors TypeScript's `_proofExcludedPnpmKeysIsExhaustive` and
/// `_proofNoContradiction`: `PNPM_CONFIG_FILE_KEYS` and `EXCLUDED_PNPM_KEYS`
/// form an exact partition of `PNPM_TYPES`, and `STRUCTURED_CONFIG_FILE_KEYS`
/// covers only structured sections outside `PNPM_TYPES` and `NPM_CONFIG_TYPES`.
#[test]
fn pnpm_types_are_partitioned_between_global_and_excluded_keys() {
    use std::collections::BTreeSet;

    let pnpm_types: BTreeSet<&str> = super::PNPM_TYPES
        .iter()
        .map(|(key, _)| *key)
        .collect();
    let npm_types: BTreeSet<&str> = super::NPM_CONFIG_TYPES
        .iter()
        .map(|(key, _)| *key)
        .collect();
    let global_keys: BTreeSet<&str> = super::routing::PNPM_CONFIG_FILE_KEYS
        .iter()
        .copied()
        .collect();
    let excluded_keys: BTreeSet<&str> = super::routing::EXCLUDED_PNPM_KEYS
        .iter()
        .copied()
        .collect();
    let structured_keys: BTreeSet<&str> = super::routing::STRUCTURED_CONFIG_FILE_KEYS
        .iter()
        .copied()
        .collect();

    assert_eq!(global_keys.len(), super::routing::PNPM_CONFIG_FILE_KEYS.len());
    assert_eq!(excluded_keys.len(), super::routing::EXCLUDED_PNPM_KEYS.len());
    assert_eq!(structured_keys.len(), super::routing::STRUCTURED_CONFIG_FILE_KEYS.len());

    let overlap: Vec<&&str> = global_keys.intersection(&excluded_keys).collect();
    assert_eq!(overlap, Vec::<&&str>::new(), "keys in both global and excluded lists");

    let union: BTreeSet<&str> = global_keys
        .union(&excluded_keys)
        .copied()
        .collect();
    assert_eq!(union, pnpm_types, "PNPM_CONFIG_FILE_KEYS ∪ EXCLUDED_PNPM_KEYS != PNPM_TYPES");

    for key in &structured_keys {
        assert!(!pnpm_types.contains(key), "structured key {key} should not be in PNPM_TYPES");
        assert!(!npm_types.contains(key), "structured key {key} should not be in NPM_CONFIG_TYPES");
        let camel = crate::naming_cases::to_camel_case(key);
        assert!(
            crate::validate_structured_setting(
                &camel,
                &serde_json::Value::String("not-an-object".to_string()),
                crate::YamlConfigTarget::Global,
            )
            .is_err(),
            "validate_structured_setting must reject scalar string for {camel}",
        );
    }
}

#[test]
fn validate_structured_setting_accepts_placeholders_and_enforces_target_trust_rules() {
    use crate::{YamlConfigTarget, validate_structured_setting};
    use serde_json::json;

    validate_structured_setting(
        "nodePackageMap",
        &json!({ "mode": "${PNPM_TEST_UNSET_MAP_MODE:-load}" }),
        YamlConfigTarget::Project,
    )
    .unwrap();
    assert!(
        validate_structured_setting(
            "nodePackageMap",
            &json!({ "mode": "${PNPM_TEST_UNSET_MAP_MODE:-bogus}" }),
            YamlConfigTarget::Project,
        )
        .is_err(),
    );

    validate_structured_setting("sideEffectsCache", &json!(true), YamlConfigTarget::Project)
        .unwrap();
    assert!(
        validate_structured_setting(
            "sideEffectsCache",
            &json!("bogus"),
            YamlConfigTarget::Project,
        )
        .is_err(),
    );

    let signed_side_effects = json!({
        "read": true,
        "remote": {
            "org": "acme",
            "packages": ["pkg"],
            "privateKey": "secret-key"
        }
    });
    let project_err = validate_structured_setting(
        "sideEffectsCache",
        &signed_side_effects,
        YamlConfigTarget::Project,
    )
    .unwrap_err();
    assert!(project_err.contains("sideEffectsCache.remote.privateKey"), "{project_err}");
    validate_structured_setting("sideEffectsCache", &signed_side_effects, YamlConfigTarget::Global)
        .unwrap();

    let remote_cache = json!({
        "url": "https://cache.example.com",
        "privateKey": "secret-key"
    });
    assert!(
        validate_structured_setting("remoteCache", &remote_cache, YamlConfigTarget::Project)
            .is_err(),
    );
    validate_structured_setting("remoteCache", &remote_cache, YamlConfigTarget::Global).unwrap();

    validate_structured_setting(
        "concurrencyGroups",
        &json!({ "db_test": 2 }),
        YamlConfigTarget::Global,
    )
    .unwrap();
    assert!(
        validate_structured_setting(
            "concurrencyGroups",
            &json!({ "../escape": 2 }),
            YamlConfigTarget::Global,
        )
        .is_err(),
    );

    assert!(
        validate_structured_setting(
            "registries",
            &json!({ "https://user:pass@registry.example.com/": { "scopes": ["@acme"] } }),
            YamlConfigTarget::Global,
        )
        .is_err(),
    );
    assert!(
        validate_structured_setting(
            "registries",
            &json!({ "https://registry.example.com/": { "_authToken": "secret" } }),
            YamlConfigTarget::Global,
        )
        .is_err(),
    );

    let env_named_registries = json!({ "corp": "${PNPM_TEST_UNSET_REG_URL}" });
    assert!(
        validate_structured_setting(
            "namedRegistries",
            &env_named_registries,
            YamlConfigTarget::Project,
        )
        .is_err(),
    );
    validate_structured_setting("namedRegistries", &env_named_registries, YamlConfigTarget::Global)
        .unwrap();

    let env_registries = json!({ "@corp": "${PNPM_TEST_UNSET_REG_URL}" });
    assert!(
        validate_structured_setting("registries", &env_registries, YamlConfigTarget::Project)
            .is_err(),
    );
    validate_structured_setting("registries", &env_registries, YamlConfigTarget::Global).unwrap();
}

#[test]
fn render_yaml_top_level_field_preserves_multiple_trailing_newlines() {
    use serde_json::{Value, json};

    for value in [
        json!("text\n\n"),
        json!("text\n\n\n"),
        json!({ "nested": "text\n\n\n" }),
        json!(["first\n\n", "second\n\n\n"]),
    ] {
        let rendered = crate::render_yaml_top_level_field("sample", &value);
        let parsed: serde_json::Map<String, Value> = serde_saphyr::from_str(&rendered).unwrap();
        assert_eq!(parsed["sample"], value, "rendered YAML:\n{rendered}");
    }
}

/// Every `WorkspaceSettings` field accepted by `is_config_file_key` must
/// survive `clear_workspace_only_fields`, and every field rejected by
/// `is_config_file_key` (except `_auth`, which routes via `is_ini_config_key`)
/// must be zeroed by `clear_workspace_only_fields`.
#[test]
fn global_config_clearing_agrees_with_config_file_key_routing() {
    let serialized_keys =
        crate::workspace_yaml::serialized_field_keys(&crate::WorkspaceSettings::default());
    let workspace_only = crate::WorkspaceSettings::workspace_only_keys();

    for key in workspace_only {
        assert!(
            serialized_keys.contains(key),
            "workspace_only_keys entry {key} does not match any serialized WorkspaceSettings field",
        );
    }

    for key in &serialized_keys {
        if key == "_auth" {
            assert!(is_ini_config_key("_auth"));
            assert!(!workspace_only.contains("_auth"));
            continue;
        }
        let kebab = crate::naming_cases::to_kebab_case(key);
        let kept_in_global_yaml = !workspace_only.contains(key);
        assert_eq!(
            kept_in_global_yaml,
            is_config_file_key(&kebab),
            "global scope mismatch for WorkspaceSettings field {key} ({kebab}): \
             kept_in_global_yaml={kept_in_global_yaml}, is_config_file_key={}",
            is_config_file_key(&kebab),
        );
    }
}
