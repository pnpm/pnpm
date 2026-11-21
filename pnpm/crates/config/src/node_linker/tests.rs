use crate::{
    Config,
    HoistingLimits,
    NodeLinker,
    TrustedSettings,
    WorkspaceSettings,
    api::EnvVar,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::Path,
};

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
        (json!({"type": "pnp"}), NodeLinker::Pnp),
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
        json!(["hoisted"]),
        json!({"type": "unknown"}),
        json!({"type": "loaded", "exclude": ["vitest"]}),
        json!({"type": "loaded", "excluded": "vitest"}),
        json!({"type": "loaded", "excluded": [false]}),
        json!({"type": "loaded", "hoist": false}),
        json!({"type": "loaded", "hoist": {"public": []}}),
        json!({"type": "hoisted", "hoistingLimits": "invalid"}),
        json!({"type": "hoisted", "hoist": false}),
        json!({"type": "isolated", "hoistingLimits": "workspaces"}),
        json!({"type": "pnp", "hoist": false}),
        json!({"type": "isolated", "hoist": {"public": [false]}}),
    ] {
        assert!(serde_json::from_value::<WorkspaceSettings>(json!({"nodeLinker": value})).is_err());
    }
}

#[test]
fn node_linker_rejects_sequence_syntax() {
    assert!(serde_saphyr::from_str::<WorkspaceSettings>("nodeLinker: [hoisted]\n").is_err());
}

#[test]
fn isolated_object_applies_private_and_public_hoist_patterns() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        "nodeLinker:\n  type: isolated\n  hoist:\n    public: []\n    private: ['*']\n",
    )
    .unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("."));
    assert_eq!(config.node_linker, NodeLinker::Isolated);
    assert_eq!(config.public_hoist_pattern, Some(vec![]));
    assert_eq!(config.hoist_pattern, Some(vec!["*".to_string()]));
}

#[test]
fn hoisted_object_applies_hoisting_limits() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("nodeLinker:\n  type: hoisted\n  hoistingLimits: workspaces\n")
            .unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("."));
    assert_eq!(config.node_linker, NodeLinker::Hoisted);
    assert_eq!(config.hoisting_limits, HoistingLimits::Workspaces);
}

#[test]
fn linker_options_round_trip_through_resolved_settings() {
    for settings_yaml in [
        "nodeLinker:\n  type: isolated\n  hoist:\n    public: ['eslint']\n    private: []\n",
        "nodeLinker:\n  type: hoisted\n  hoistingLimits: dependencies\n",
    ] {
        let settings: WorkspaceSettings = serde_saphyr::from_str(settings_yaml).unwrap();
        let mut config = Config::default();
        settings.apply_to(&mut config, Path::new("."));
        let reported = serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap();
        let mut restored = Config::default();
        serde_json::from_value::<WorkspaceSettings>(reported)
            .unwrap()
            .apply_to(&mut restored, Path::new("."));
        assert_eq!(restored.node_linker, config.node_linker);
        assert_eq!(restored.hoist_pattern, config.hoist_pattern);
        assert_eq!(restored.public_hoist_pattern, config.public_hoist_pattern);
        assert_eq!(restored.hoisting_limits, config.hoisting_limits);
    }
}

#[test]
fn public_only_hoisting_survives_a_resolved_settings_round_trip() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("hoist: false\npublicHoistPattern: ['eslint']\n").unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("."));
    let reported = serde_json::to_value(WorkspaceSettings::from_resolved(&config)).unwrap();
    let mut restored = Config::default();
    serde_json::from_value::<WorkspaceSettings>(reported)
        .unwrap()
        .apply_to(&mut restored, Path::new("."));
    assert_eq!(restored.public_hoist_pattern, Some(vec!["eslint".to_string()]));
    assert!(restored.hoist_pattern.unwrap_or_default().is_empty());
}

#[test]
fn linker_options_under_virtual_store_only_go_to_the_restored_patterns() {
    let mut config = Config { virtual_store_only: true, ..Config::default() };
    config.apply_virtual_store_only_derivation();
    serde_saphyr::from_str::<WorkspaceSettings>(
        "nodeLinker:\n  type: isolated\n  hoist:\n    public: ['eslint']\n",
    )
    .unwrap()
    .apply_to(&mut config, Path::new("."));
    assert_eq!(config.public_hoist_pattern, Some(vec![]));
    config.virtual_store_only = false;
    config.restore_hoist_patterns_after_virtual_store_only();
    assert_eq!(config.public_hoist_pattern, Some(vec!["eslint".to_string()]));
}

#[test]
fn resetting_node_linker_drops_its_patterns_from_the_virtual_store_only_snapshot() {
    let defaults = Config::default();
    let mut config = Config { virtual_store_only: true, ..Config::default() };
    serde_saphyr::from_str::<WorkspaceSettings>(
        "nodeLinker:\n  type: isolated\n  hoist:\n    public: ['eslint']\n",
    )
    .unwrap()
    .apply_to(&mut config, Path::new("."));
    config.apply_virtual_store_only_derivation();
    config.explicit_settings.remove("nodeLinker");
    WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &defaults,
        "nodeLinker",
        Path::new("."),
    );
    config.virtual_store_only = false;
    config.restore_hoist_patterns_after_virtual_store_only();
    assert_eq!(config.public_hoist_pattern, defaults.public_hoist_pattern);
}

#[test]
fn object_options_win_over_flat_aliases_from_the_same_source() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        "nodeLinker:\n  type: isolated\n  hoist: false\nhoistPattern: ['legacy']\npublicHoistPattern: ['legacy']\n",
    )
    .unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("."));
    assert_eq!(config.hoist_pattern, None);
    assert_eq!(config.public_hoist_pattern, None);
}

#[test]
fn higher_priority_flat_setting_overrides_a_lower_priority_linker_option() {
    let mut config = Config::default();
    serde_saphyr::from_str::<WorkspaceSettings>(
        "nodeLinker:\n  type: isolated\n  hoist:\n    public: ['lower']\n",
    )
    .unwrap()
    .apply_to(&mut config, Path::new("."));
    serde_saphyr::from_str::<WorkspaceSettings>("publicHoistPattern: ['higher']\n")
        .unwrap()
        .apply_to(&mut config, Path::new("."));
    assert_eq!(config.public_hoist_pattern, Some(vec!["higher".to_string()]));
}

#[test]
fn null_linker_options_keep_values_from_lower_priority_sources() {
    let mut config = Config {
        node_linker: NodeLinker::Isolated,
        hoist_pattern: Some(vec!["private".to_string()]),
        public_hoist_pattern: Some(vec!["public".to_string()]),
        ..Config::default()
    };
    serde_saphyr::from_str::<WorkspaceSettings>("nodeLinker:\n  type: isolated\n  hoist: null\n")
        .unwrap()
        .apply_to(&mut config, Path::new("."));
    assert_eq!(config.hoist_pattern, Some(vec!["private".to_string()]));
    assert_eq!(config.public_hoist_pattern, Some(vec!["public".to_string()]));
}

#[test]
fn resetting_node_linker_restores_its_options_and_keeps_flat_aliases() {
    let defaults = Config::default();
    let mut config = Config::default();
    serde_saphyr::from_str::<WorkspaceSettings>(
        "nodeLinker:\n  type: hoisted\n  hoistingLimits: workspaces\n",
    )
    .unwrap()
    .apply_to(&mut config, Path::new("."));
    config.explicit_settings.insert("hoistPattern".to_string(), json!(["legacy-pattern"]));
    config.explicit_settings.insert("publicHoistPattern".to_string(), json!(["legacy-public"]));
    config.explicit_settings.remove("nodeLinker");
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &defaults,
        "nodeLinker",
        Path::new("."),
    ));
    assert_eq!(config.node_linker, defaults.node_linker);
    assert_eq!(config.hoisting_limits, defaults.hoisting_limits);
    assert_eq!(config.hoist_pattern, Some(vec!["legacy-pattern".to_string()]));
    assert_eq!(config.public_hoist_pattern, Some(vec!["legacy-public".to_string()]));
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
        TrustedSettings::from_string_values(&BTreeMap::from([(
            "node-linker".to_string(),
            value.to_string(),
        )]))
        .settings
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
    TrustedSettings::from_pnpm_config_env::<Environment>().settings
        .apply_to(&mut config, Path::new("."));
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
fn switching_loaded_linker_restores_global_virtual_store_and_keeps_modules_dir() {
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
        assert_eq!(config.modules_dir, original);
        assert_eq!(config.store_loader_dir(Path::new("/project")), original.join(".pnpm"));
        config.node_linker = NodeLinker::Isolated;
        config.apply_global_virtual_store_derivation(false, false);
        assert_eq!(config.modules_dir, original);
        assert_eq!(config.enable_global_virtual_store, explicit);
        assert_eq!(config.install_state_dir, original.join(".pnpm"));
    }
}
