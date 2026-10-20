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
        json!({"type": "isolated", "hoist": {"public": []}}),
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
