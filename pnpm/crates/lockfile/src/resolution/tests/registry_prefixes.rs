use crate::{
    DeclaredRegistryPrefixes, PkgNameVerPeer, RegistryContext, normalized_registry_key,
    same_registry,
};
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, HashMap};

fn declared(entries: &[(&str, &str)]) -> DeclaredRegistryPrefixes {
    DeclaredRegistryPrefixes::new(entries.iter().copied())
}

fn key(key: &str) -> PkgNameVerPeer {
    key.parse().expect("parse key")
}

#[test]
fn normalized_registry_key_lowercases_scheme_and_host_and_ends_in_a_slash() {
    assert_eq!(
        normalized_registry_key("HTTPS://NPM.Example.com/Team-A").as_deref(),
        Some("https://npm.example.com/Team-A/"),
    );
    assert_eq!(
        normalized_registry_key("http://127.0.0.1:8080").as_deref(),
        Some("http://127.0.0.1:8080/"),
    );
    assert_eq!(normalized_registry_key("not a url"), None);
}

#[test]
fn same_registry_compares_normalized_or_else_as_written() {
    assert!(same_registry("HTTPS://npm.example.com", "https://npm.example.com/"));
    assert!(!same_registry("https://npm.example.com/a/", "https://npm.example.com/A/"));
    assert!(same_registry("not a url", "not a url"));
    assert!(!same_registry("not a url", "https://npm.example.com/"));
}

#[test]
fn prefix_is_found_under_every_spelling_of_its_registry() {
    let prefixes = declared(&[("work", "https://npm.example.com")]);
    for spelling in
        ["https://npm.example.com/", "HTTPS://NPM.Example.com", "https://npm.example.com"]
    {
        assert_eq!(prefixes.prefix_for(spelling), Some("work"), "{spelling}");
    }
    assert_eq!(prefixes.prefix_for("https://npm.example.com/sub/"), None);
    assert_eq!(prefixes.prefix_for("https://other.example.com/"), None);
}

#[test]
fn several_prefixes_for_one_registry_resolve_to_the_smallest() {
    let prefixes = declared(&[
        ("zeta", "https://npm.example.com/"),
        ("work", "https://npm.example.com"),
        ("alpha", "https://other.example.com/"),
    ]);
    assert_eq!(prefixes.prefix_for("https://npm.example.com/"), Some("work"));
    assert_eq!(prefixes.prefix_for("https://other.example.com/"), Some("alpha"));
}

#[test]
fn an_unparsable_registry_declares_nothing() {
    let prefixes = declared(&[("broken", "not a url")]);
    assert!(prefixes.is_empty());
    assert_eq!(prefixes.prefix_for("not a url"), None);
}

/// `work` and `zeta` both name the registry the `@work` scope routes to;
/// `gh` is the built-in alias, which nothing declares.
fn context() -> RegistryContext {
    let registry = "https://npm.example.com/";
    RegistryContext {
        registries: HashMap::from([
            ("default".to_string(), "https://registry.npmjs.org/".to_string()),
            ("@work".to_string(), registry.to_string()),
        ]),
        registries_by_prefix: HashMap::from([
            ("gh".to_string(), "https://npm.pkg.github.com/".to_string()),
            ("work".to_string(), "https://npm.example.com".to_string()),
            ("zeta".to_string(), registry.to_string()),
        ]),
        declared_prefixes: declared(&[("work", "https://npm.example.com"), ("zeta", registry)]),
        registry_options_by_url: BTreeMap::default(),
    }
}

#[test]
fn key_prefix_for_alias_is_the_prefix_its_registry_keys_carry() {
    let context = context();
    assert_eq!(context.key_prefix_for_alias("work"), Some("work"));
    assert_eq!(context.key_prefix_for_alias("zeta"), Some("work"));
    assert_eq!(context.key_prefix_for_alias("gh"), Some("gh"));
    assert_eq!(context.key_prefix_for_alias("gone"), None);
}

#[test]
fn a_plain_key_is_rekeyed_by_the_prefix_its_scope_routes_to() {
    let context = context();
    assert_eq!(context.rekeyed(&key("@work/foo@1.0.0")), Some(key("@work/foo@work:1.0.0")));
    assert_eq!(
        context.rekeyed(&key("@work/foo@1.0.0(peer@2.0.0)")),
        Some(key("@work/foo@work:1.0.0(peer@2.0.0)")),
    );
    assert_eq!(context.rekeyed(&key("foo@1.0.0")), None, "the default registry declares no prefix");
}

#[test]
fn a_qualified_key_is_rekeyed_only_by_another_prefix_of_its_registry() {
    let context = context();
    assert_eq!(context.rekeyed(&key("@work/foo@work:1.0.0")), None);
    assert_eq!(context.rekeyed(&key("@work/foo@zeta:1.0.0")), Some(key("@work/foo@work:1.0.0")));
    assert_eq!(context.rekeyed(&key("foo@gh:1.0.0")), None, "a built-in alias nothing declares");
    assert_eq!(
        context.rekeyed(&key("foo@gone:1.0.0")),
        None,
        "an undeclared alias is the install's to report",
    );
}

#[test]
fn keys_without_a_registry_version_are_never_rekeyed() {
    let context = context();
    assert_eq!(context.rekeyed(&key("@work/foo@file:packages/foo")), None);
    assert_eq!(context.rekeyed(&key("@work/node@runtime:22.0.0")), None);
}
