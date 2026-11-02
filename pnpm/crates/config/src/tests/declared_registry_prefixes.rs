use super::{Config, assert_eq};

fn config_with_prefixes(prefixes: &[(&str, &str)]) -> Config {
    Config {
        registries_by_prefix: prefixes
            .iter()
            .map(|(prefix, registry)| ((*prefix).to_string(), (*registry).to_string()))
            .collect(),
        ..Config::default()
    }
}

/// <https://github.com/pnpm/pnpm/issues/17575>
#[test]
fn a_re_pointed_builtin_alias_qualifies_no_keys() {
    let config = config_with_prefixes(&[
        ("npmjs", "https://mirror.example.com/"),
        ("gh", "https://registry.npmjs.org/"),
    ]);

    let declared = config.declared_registry_prefixes();

    assert!(declared.is_empty());
    assert_eq!(
        config.registry_context().registry_of_prefix("npmjs"),
        Some("https://mirror.example.com/"),
    );
}

#[test]
fn a_custom_prefix_beside_a_builtin_alias_keys_its_registry() {
    let config = config_with_prefixes(&[
        ("npmjs", "https://mirror.example.com/"),
        ("work", "https://mirror.example.com/"),
    ]);

    assert_eq!(
        config.declared_registry_prefixes().prefix_for("https://mirror.example.com/"),
        Some("work"),
    );
}
