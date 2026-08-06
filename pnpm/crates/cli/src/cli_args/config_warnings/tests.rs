use super::take_unemitted;
use pacquet_config::Config;
use pretty_assertions::assert_eq;
use std::collections::HashSet;

#[test]
fn a_repeated_config_load_reports_each_warning_once() {
    let mut emitted = HashSet::new();

    let mut first = Config { config_warnings: vec![warn("a"), warn("b")], ..Config::new() };
    assert_eq!(take_unemitted(&mut emitted, &mut first), vec![warn("a"), warn("b")]);
    assert!(first.config_warnings.is_empty(), "the drained config keeps no warnings");

    let mut reload =
        Config { config_warnings: vec![warn("a"), warn("b"), warn("c")], ..Config::new() };
    assert_eq!(take_unemitted(&mut emitted, &mut reload), vec![warn("c")]);
}

fn warn(id: &str) -> String {
    format!("warning {id}")
}
