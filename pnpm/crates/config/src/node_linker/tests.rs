use crate::{Config, NodeLinker, WorkspaceSettings, api::EnvVar};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

#[test]
fn loaded_object_applies_exclusions_and_round_trips_through_resolved_settings() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        "nodeLinker:\n  type: loaded\n  excluded:\n    - vitest\n    - esbuild\n",
    )
    .unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("."));
    assert_eq!(config.node_linker, NodeLinker::Loaded);
    assert_eq!(config.node_linker_excluded, ["vitest", "esbuild"]);
    let reported = serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap();
    assert_eq!(
        reported["nodeLinker"],
        json!({"type": "loaded", "excluded": ["vitest", "esbuild"]}),
    );
    assert!(reported.get("casMaterialize").is_none());
    let mut restored = Config::default();
    serde_json::from_value::<WorkspaceSettings>(reported)
        .unwrap()
        .apply_to(&mut restored, Path::new("."));
    assert_eq!(restored.node_linker, config.node_linker);
    assert_eq!(restored.node_linker_excluded, config.node_linker_excluded);
}

#[test]
fn scalar_linkers_and_loaded_object_without_exclusions_are_supported() {
    for (value, expected) in [
        (json!("isolated"), NodeLinker::Isolated),
        (json!("hoisted"), NodeLinker::Hoisted),
        (json!("pnp"), NodeLinker::Pnp),
        (json!("loaded"), NodeLinker::Loaded),
        (json!({"type": "loaded"}), NodeLinker::Loaded),
    ] {
        let settings: WorkspaceSettings =
            serde_json::from_value(json!({"nodeLinker": value})).unwrap();
        let mut config = Config::default();
        settings.apply_to(&mut config, Path::new("."));
        assert_eq!(config.node_linker, expected);
        assert!(config.node_linker_excluded.is_empty());
    }
}

#[test]
fn node_linker_objects_reject_invalid_and_not_yet_supported_options() {
    for value in [
        json!({"excluded": ["vitest"]}),
        json!({"type": "unknown"}),
        json!({"type": "loaded", "exclude": ["vitest"]}),
        json!({"type": "loaded", "excluded": "vitest"}),
        json!({"type": "loaded", "excluded": [false]}),
        json!({"type": "loaded", "hoist": false}),
        json!({"type": "isolated", "hoist": {"unknown": []}}),
    ] {
        assert!(serde_json::from_value::<WorkspaceSettings>(json!({"nodeLinker": value})).is_err());
    }
}

#[test]
fn higher_priority_linker_values_replace_the_type_and_exclusions_together() {
    let mut config = Config::default();
    for (value, expected, excluded) in [
        (r#"{"type":"loaded","excluded":["vitest"]}"#, NodeLinker::Loaded, vec!["vitest"]),
        (r#"{"type":"loaded","excluded":["esbuild"]}"#, NodeLinker::Loaded, vec!["esbuild"]),
        (r#"{"type":"loaded","excluded":[]}"#, NodeLinker::Loaded, vec![]),
        (r#"{"type":"loaded","excluded":["vitest"]}"#, NodeLinker::Loaded, vec!["vitest"]),
        ("hoisted", NodeLinker::Hoisted, vec![]),
        ("loaded", NodeLinker::Loaded, vec![]),
    ] {
        WorkspaceSettings::from_string_values(&BTreeMap::from([(
            "node-linker".to_string(),
            value.to_string(),
        )]))
        .apply_to(&mut config, Path::new("."));
        assert_eq!(config.node_linker, expected);
        assert_eq!(config.node_linker_excluded, excluded);
    }
}

#[test]
fn environment_accepts_loaded_object_and_reset_clears_exclusions() {
    struct Environment;
    impl EnvVar for Environment {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_NODE_LINKER").then(|| {
                r#"{"type":"loaded","excluded":["vitest"]}"#.to_string()
            })
        }
    }
    let defaults = Config::default();
    let mut config = Config::default();
    WorkspaceSettings::from_pnpm_config_env::<Environment>().apply_to(&mut config, Path::new("."));
    assert_eq!(config.node_linker, NodeLinker::Loaded);
    assert_eq!(config.node_linker_excluded, ["vitest"]);
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &defaults,
        "nodeLinker",
        Path::new("."),
    ));
    assert_eq!(config.node_linker, defaults.node_linker);
    assert!(config.node_linker_excluded.is_empty());
}

#[test]
fn switching_loaded_linker_restores_layout_defaults_and_preserves_explicit_settings() {
    for explicit in [false, true] {
        let original =
            Path::new("/project").join(if explicit { "custom-modules" } else { "node_modules" });
        let mut config = Config {
            modules_dir: original.clone(),
            enable_global_virtual_store: explicit,
            ..Config::default()
        };
        if explicit {
            config.explicit_settings.insert("modulesDir".into(), json!("custom-modules"));
            config.explicit_settings.insert("enableGlobalVirtualStore".into(), json!(true));
        }
        config.node_linker = NodeLinker::Loaded;
        config.apply_global_virtual_store_derivation(false, false);
        assert!(config.enable_global_virtual_store);
        assert_eq!(
            config.modules_dir,
            if explicit { original.clone() } else { Path::new("/project").join(".pnpm") },
        );
        config.node_linker = NodeLinker::Isolated;
        config.apply_global_virtual_store_derivation(false, false);
        assert_eq!(config.modules_dir, original);
        assert_eq!(config.enable_global_virtual_store, explicit);
        assert_eq!(config.install_state_dir, original.join(".pnpm"));
    }
}

#[test]
fn isolated_hoist_options_merge_per_field_and_reset_explicit_values() {
    let mut config = Config::default();
    for (input, public, private) in [
        (
            json!({"shamefullyHoist": true, "nodeLinker": {"type": "isolated", "hoist": {"public": ["foo"], "private": ["*", "!foo"]}}}),
            vec!["foo"],
            vec!["*", "!foo"],
        ),
        (
            json!({"nodeLinker": {"type": "isolated", "hoist": {"public": []}}}),
            vec![],
            vec!["*", "!foo"],
        ),
        (json!({"nodeLinker": {"type": "isolated", "hoist": false}}), vec![], vec![]),
        (json!({"nodeLinker": {"type": "isolated", "hoist": null}}), vec![], vec!["*"]),
        (
            json!({"nodeLinker": {"type": "isolated", "hoist": {"public": null, "private": null}}}),
            vec![],
            vec!["*"],
        ),
    ] {
        let settings: WorkspaceSettings = serde_json::from_value(input).unwrap();
        config.record_explicit_settings(&settings);
        settings.apply_to(&mut config, Path::new("."));
        assert_eq!(config.public_hoist_pattern.as_deref().unwrap_or_default(), public);
        assert_eq!(config.hoist_pattern.as_deref().unwrap_or_default(), private);
    }
    let value = serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap();
    assert_eq!(
        value["nodeLinker"],
        json!({"type":"isolated", "hoist":{"public":[], "private":["*"]}}),
    );
}

#[test]
fn hoisted_limits_nested_alias_and_string_sources_round_trip() {
    let mut config = Config::default();
    let settings = WorkspaceSettings::from_string_values(&BTreeMap::from([
        ("node-linker".into(), r#"{"type":"hoisted","hoistingLimits":"workspaces"}"#.into()),
        ("hoisting-limits".into(), "dependencies".into()),
    ]));
    config.record_explicit_settings(&settings);
    settings.apply_to(&mut config, Path::new("."));
    assert_eq!(config.hoisting_limits, crate::HoistingLimits::Workspaces);
    let value = serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap();
    assert_eq!(value["nodeLinker"], json!({"type":"hoisted", "hoistingLimits":"workspaces"}));
    serde_json::from_value::<WorkspaceSettings>(
        json!({"nodeLinker":{"type":"hoisted","hoistingLimits":null}}),
    )
    .unwrap()
    .apply_to(&mut config, Path::new("."));
    assert_eq!(config.hoisting_limits, crate::HoistingLimits::default());
}

#[test]
fn environment_scoped_hoisting_overrides_lower_priority_flat_aliases() {
    struct Environment;
    impl EnvVar for Environment {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_NODE_LINKER").then(|| {
                r#"{"type":"isolated","hoist":{"public":[],"private":["bar"]}}"#.into()
            })
        }
    }
    let mut config = Config::default();
    serde_json::from_value::<WorkspaceSettings>(json!({"shamefullyHoist":true}))
        .unwrap()
        .apply_to(&mut config, Path::new("."));
    WorkspaceSettings::from_pnpm_config_env::<Environment>().apply_to(&mut config, Path::new("."));
    config.apply_shamefully_hoist_derivation();
    assert_eq!(config.public_hoist_pattern, Some(vec![]));
    assert_eq!(config.hoist_pattern, Some(vec!["bar".into()]));
    config.record_explicit_settings(&WorkspaceSettings::from_pnpm_config_env::<Environment>());
    assert_eq!(config.explicit_settings["publicHoistPattern"], json!([]));
}

#[test]
fn resolved_linker_does_not_reenable_legacy_disabled_hoisting() {
    let config = Config { hoist: false, ..Config::default() };
    let mut restored = Config::default();
    WorkspaceSettings::from_resolved(&config).apply_to(&mut restored, Path::new("."));
    assert!(restored.hoist_pattern.as_ref().is_none_or(Vec::is_empty));
}

#[test]
fn public_only_hoisting_preserves_disabled_private_hoisting() {
    for lower in [json!({"hoist":false}), json!({"nodeLinker":{"type":"isolated","hoist":false}})] {
        let mut config = Config::default();
        serde_json::from_value::<WorkspaceSettings>(lower)
            .unwrap()
            .apply_to(&mut config, Path::new("."));
        let higher: WorkspaceSettings = serde_json::from_value(
            json!({"nodeLinker":{"type":"isolated","hoist":{"public":["foo"]}}}),
        )
        .unwrap();
        config.record_explicit_settings(&higher);
        higher.apply_to(&mut config, Path::new("."));
        assert!(!config.hoist);
        assert!(config.hoist_pattern.as_ref().is_none_or(Vec::is_empty));
        assert_eq!(config.public_hoist_pattern, Some(vec!["foo".into()]));
        let resolved = serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap();
        assert_eq!(resolved["nodeLinker"]["hoist"]["private"], json!([]));
        let private: WorkspaceSettings = serde_json::from_value(
            json!({"nodeLinker":{"type":"isolated","hoist":{"private":["bar"]}}}),
        )
        .unwrap();
        private.apply_to(&mut config, Path::new("."));
        assert!(config.hoist);
        assert_eq!(config.hoist_pattern, Some(vec!["bar".into()]));
        assert_eq!(config.public_hoist_pattern, Some(vec!["foo".into()]));
    }
}

#[test]
fn omitted_linker_options_remain_omitted_when_serialized() {
    for value in [json!({"type":"isolated"}), json!({"type":"hoisted"})] {
        let linker: crate::NodeLinkerSetting = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(linker).unwrap(), value);
    }
    for value in
        [json!({"type":"isolated","hoist":null}), json!({"type":"hoisted","hoistingLimits":null})]
    {
        let linker: crate::NodeLinkerSetting = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(linker).unwrap(), value);
    }
}

#[test]
fn resolved_hook_linker_shape_preserves_scalar_and_object_selection() {
    for linker in [NodeLinker::Isolated, NodeLinker::Hoisted, NodeLinker::Pnp] {
        let config = Config { node_linker: linker, ..Config::default() };
        assert_eq!(
            serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap()["nodeLinker"],
            serde_json::to_value(linker).unwrap(),
        );
    }
    for (value, expected) in [
        (
            json!({"type":"isolated"}),
            json!({"type":"isolated","hoist":{"public":[],"private":["*"]}}),
        ),
        (json!({"type":"hoisted"}), json!({"type":"hoisted","hoistingLimits":"none"})),
        (json!({"type":"loaded","excluded":[]}), json!({"type":"loaded","excluded":[]})),
    ] {
        let settings: WorkspaceSettings =
            serde_json::from_value(json!({"nodeLinker":value})).unwrap();
        let mut config = Config::default();
        config.record_explicit_settings(&settings);
        settings.apply_to(&mut config, Path::new("."));
        assert_eq!(
            serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap()["nodeLinker"],
            expected,
        );
    }
}
