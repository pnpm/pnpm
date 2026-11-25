use super::drop_cli_map_policies;
use pnpm_config::Config;
use pretty_assertions::assert_eq;
use serde_json::{
    Map,
    Value,
    json,
};

#[test]
fn cli_catalog_policy_preserves_hook_entries_and_drops_both_policy_spellings() {
    let config = Config { cli_settings: ["catalogPrune".to_owned()].into(), ..Config::default() };
    let mut changes = Map::from_iter([
        ("catalog".to_owned(), json!({"entries": {"example": "1"}, "unused": "prune"})),
        ("cleanupUnusedCatalogs".to_owned(), json!(true)),
    ]);
    drop_cli_map_policies(&config, &mut changes);
    assert_eq!(Value::Object(changes), json!({"catalog":{"entries":{"example":"1"}}}));
}

#[test]
fn a_cli_rules_section_protects_its_policy_from_the_hook_alias() {
    let config = Config {
        cli_settings: ["overrides".to_owned()].into(),
        explicit_settings: Map::from_iter([(
            "overrides".to_owned(),
            json!({"rules": {}, "unused": "ignore"}),
        )]),
        ..Config::default()
    };
    let mut changes = Map::from_iter([
        ("warnUnusedOverrides".to_owned(), json!(true)),
        ("catalogPrune".to_owned(), json!(true)),
    ]);
    drop_cli_map_policies(&config, &mut changes);
    assert_eq!(Value::Object(changes), json!({"catalogPrune": true}));
}

#[test]
fn catalog_options_respect_cli_choices_and_supersede_hook_aliases() {
    let config = Config { cli_settings: ["catalogMode".to_owned()].into(), ..Config::default() };
    let mut changes = Map::from_iter([
        ("catalog".to_owned(), json!({"mode":"strict", "saveTo":"tools", "entries":{}})),
        ("saveCatalogName".to_owned(), Value::Null),
    ]);
    drop_cli_map_policies(&config, &mut changes);
    assert_eq!(Value::Object(changes), json!({"catalog":{"entries":{}, "saveTo":"tools"}}));
}
