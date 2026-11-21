use super::{
    drop_cli_linker_aliases,
    reapply_linker_options,
};
use pnpm_config::Config;
use pretty_assertions::assert_eq;
use serde_json::{
    Map,
    Value,
    json,
};

fn delta(value: Value) -> Map<String, Value> {
    let Value::Object(map) = value else { panic!("delta must be an object") };
    map
}

#[test]
fn a_cli_flat_setting_protects_the_matching_linker_option() {
    let config =
        Config { cli_settings: ["publicHoistPattern".to_string()].into(), ..Config::default() };
    let mut changes = delta(json!({
        "nodeLinker": {"type": "isolated", "hoist": {"public": ["hook"], "private": ["hook"]}},
    }));
    drop_cli_linker_aliases(&config, &mut changes);
    assert_eq!(
        Value::Object(changes),
        json!({"nodeLinker": {"type": "isolated", "hoist": {"private": ["hook"]}}}),
    );
}

#[test]
fn a_cli_flat_setting_protects_against_a_boolean_hoist() {
    let config = Config { cli_settings: ["hoistPattern".to_string()].into(), ..Config::default() };
    let mut changes = delta(json!({"nodeLinker": {"type": "isolated", "hoist": false}}));
    drop_cli_linker_aliases(&config, &mut changes);
    assert_eq!(Value::Object(changes), json!({"nodeLinker": {"type": "isolated"}}));
}

#[test]
fn a_cli_linker_object_protects_the_flat_aliases_it_sets() {
    let config = Config {
        cli_settings: ["nodeLinker".to_string()].into(),
        explicit_settings: Map::from_iter([(
            "nodeLinker".to_string(),
            json!({"type": "isolated", "hoist": {"public": ["cli"]}}),
        )]),
        ..Config::default()
    };
    let mut changes = delta(json!({
        "publicHoistPattern": ["hook"],
        "shamefullyHoist": true,
        "hoistPattern": ["hook"],
    }));
    drop_cli_linker_aliases(&config, &mut changes);
    assert_eq!(Value::Object(changes), json!({"hoistPattern": ["hook"]}));
}

#[test]
fn linker_options_win_over_a_flat_alias_the_same_hook_nulled() {
    let mut config = Config { hoist_pattern: Some(vec!["*".to_string()]), ..Config::default() };
    reapply_linker_options(
        &mut config,
        &json!({"hoistPattern": null, "nodeLinker": {"type": "isolated", "hoist": {"private": ["selected"]}}}),
    );
    assert_eq!(config.hoist_pattern, Some(vec!["selected".to_string()]));
}
