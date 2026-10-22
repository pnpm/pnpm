use super::{Config, assert_eq};

const WORK: &str = "https://npm.work.example.com/";

fn config_with_prefixes(prefixes: &[(&str, &str)]) -> Config {
    let mut config = Config::new();
    config.registries_by_prefix = prefixes
        .iter()
        .map(|(prefix, url)| (prefix.to_string(), url.to_string()))
        .collect();
    config
}

#[test]
fn a_registry_matches_its_prefix_regardless_of_trailing_slash_and_host_case() {
    let config = config_with_prefixes(&[("work", "https://NPM.Work.Example.com")]);
    assert_eq!(config.declared_registry_prefix(WORK), Some("work"));
    assert_eq!(config.declared_registry_prefix("HTTPS://npm.work.example.com"), Some("work"));
    assert_eq!(config.declared_registry_prefix("https://npm.other.example.com/"), None);
}

#[test]
fn a_registry_path_keeps_its_case() {
    let config = config_with_prefixes(&[("work", "https://registry.example.com/Work/")]);
    assert_eq!(config.declared_registry_prefix("https://registry.example.com/work/"), None);
    assert_eq!(config.declared_registry_prefix("https://registry.example.com/Work"), Some("work"));
}

#[test]
fn the_alphabetically_first_of_several_prefixes_on_one_registry_is_answered() {
    let config = config_with_prefixes(&[("zeta", WORK), ("alpha", WORK), ("mid", WORK)]);
    assert_eq!(config.declared_registry_prefix(WORK), Some("alpha"));
}

#[test]
fn the_built_in_prefixes_are_not_declared() {
    assert_eq!(Config::new().declared_registry_prefix("https://registry.npmjs.org/"), None);
    assert_eq!(Config::new().declared_registry_prefix("https://npm.pkg.github.com/"), None);
}
