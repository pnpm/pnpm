use std::collections::HashMap;

use pretty_assertions::assert_eq;

use super::{
    MergeNamedRegistriesError, build_named_registry_prefixes, merge_named_registries,
    pick_registry_for_package, pick_registry_for_version,
};

fn registries(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}

#[test]
fn build_prefixes_includes_gh_builtin() {
    let prefixes = build_named_registry_prefixes(&HashMap::new());
    assert!(prefixes.iter().any(|prefix| prefix == "https://npm.pkg.github.com/"));
}

#[test]
fn build_prefixes_overrides_builtin_on_same_key() {
    let mut named = HashMap::new();
    named.insert("gh".to_string(), "https://internal/gh/".to_string());
    let prefixes = build_named_registry_prefixes(&named);
    assert!(prefixes.iter().any(|prefix| prefix == "https://internal/gh/"));
    assert!(!prefixes.iter().any(|prefix| prefix == "https://npm.pkg.github.com/"));
}

#[test]
fn build_prefixes_sorts_longest_first() {
    let mut named = HashMap::new();
    named.insert("a".to_string(), "https://npm.example/team-a".to_string());
    named.insert("b".to_string(), "https://npm.example/team-a/sub".to_string());
    let prefixes = build_named_registry_prefixes(&named);
    assert!(
        prefixes[0].starts_with("https://npm.example/team-a/sub"),
        "longest prefix first, got {prefixes:?}",
    );
}

#[test]
fn tarball_under_named_registry_wins_over_scope_routing() {
    let prefixes = build_named_registry_prefixes(&HashMap::new());
    let regs = registries(&[("default", "https://registry.npmjs.org/")]);
    let picked = pick_registry_for_version(
        &regs,
        &prefixes,
        "@scope/foo",
        Some("https://npm.pkg.github.com/@scope/foo/-/foo-1.0.0.tgz"),
    );
    assert_eq!(picked, "https://npm.pkg.github.com/");
}

#[test]
fn falls_back_to_scope_routing_without_tarball() {
    let regs = registries(&[
        ("default", "https://registry.npmjs.org/"),
        ("@private", "https://internal/registry/"),
    ]);
    let prefixes = build_named_registry_prefixes(&HashMap::new());

    let scoped = pick_registry_for_version(&regs, &prefixes, "@private/foo", None);
    assert_eq!(scoped, "https://internal/registry/");

    let bare = pick_registry_for_version(&regs, &prefixes, "lodash", None);
    assert_eq!(bare, "https://registry.npmjs.org/");
}

/// Without consulting the `npm:` target's scope, an npm-alias entry
/// like `"foo": "npm:@acme/bar@^1"` would route through the empty
/// scope of `foo` and miss the user's `registries[@acme]` override.
#[test]
fn npm_alias_uses_bare_specifier_scope_over_local_name() {
    let regs = registries(&[
        ("default", "https://registry.npmjs.org/"),
        ("@acme", "https://npm.acme.example/"),
    ]);
    let picked = pick_registry_for_package(&regs, "foo", Some("npm:@acme/bar@^1"));
    assert_eq!(picked, "https://npm.acme.example/");
}

#[test]
fn falls_back_to_pkg_name_scope_without_npm_alias() {
    let regs = registries(&[
        ("default", "https://registry.npmjs.org/"),
        ("@private", "https://internal/registry/"),
    ]);
    let picked = pick_registry_for_package(&regs, "@private/foo", Some("^1.0.0"));
    assert_eq!(picked, "https://internal/registry/");
}

#[test]
fn scoped_npm_alias_target_in_different_scope_wins_over_local() {
    let regs = registries(&[
        ("default", "https://registry.npmjs.org/"),
        ("@scope1", "https://scope1.registry/"),
        ("@scope2", "https://scope2.registry/"),
    ]);
    let picked = pick_registry_for_package(&regs, "@scope1/foo", Some("npm:@scope2/bar@^1.0.0"));
    assert_eq!(picked, "https://scope2.registry/");
}

#[test]
fn unscoped_npm_alias_target_routes_to_default() {
    let regs = registries(&[
        ("default", "https://registry.npmjs.org/"),
        ("@private", "https://internal/registry/"),
    ]);
    let picked = pick_registry_for_package(&regs, "@private/foo", Some("npm:lodash@^1"));
    assert_eq!(picked, "https://registry.npmjs.org/");
}

#[test]
fn merge_includes_builtin_when_user_empty() {
    let merged = merge_named_registries(&HashMap::new()).unwrap();
    assert_eq!(merged.get("gh").map(String::as_str), Some("https://npm.pkg.github.com/"));
}

/// `npmjs` reaches the public registry even when `registry` points at
/// an internal proxy. The `npm` prefix cannot: it is reserved for the
/// alias protocol and routes through the default registry.
#[test]
fn merge_includes_builtin_npmjs() {
    let merged = merge_named_registries(&HashMap::new()).unwrap();
    assert_eq!(merged.get("npmjs").map(String::as_str), Some("https://registry.npmjs.org/"));
}

/// A proxying org repoints `npmjs` so a recorded npmjs tarball URL
/// keeps verifying against their proxy instead of the public host.
#[test]
fn merge_user_overrides_builtin_npmjs() {
    let mut user = HashMap::new();
    user.insert("npmjs".to_string(), "https://npm.proxy.example/".to_string());
    let merged = merge_named_registries(&user).unwrap();
    assert_eq!(merged.get("npmjs").map(String::as_str), Some("https://npm.proxy.example/"));
    let prefixes = build_named_registry_prefixes(&user);
    assert!(prefixes.iter().any(|prefix| prefix == "https://npm.proxy.example/"));
    assert!(!prefixes.iter().any(|prefix| prefix == "https://registry.npmjs.org/"));
}

#[test]
fn merge_user_overrides_builtin_gh() {
    let mut user = HashMap::new();
    user.insert("gh".to_string(), "https://npm.ghes.example.com/".to_string());
    let merged = merge_named_registries(&user).unwrap();
    assert_eq!(merged.get("gh").map(String::as_str), Some("https://npm.ghes.example.com/"));
}

#[test]
fn merge_rejects_url_without_scheme() {
    let mut user = HashMap::new();
    user.insert("work".to_string(), "npm.work.example.com".to_string());
    let err = merge_named_registries(&user).expect_err("missing scheme must error");
    assert!(matches!(err, MergeNamedRegistriesError::InvalidUrl { .. }), "got {err:?}");
}

#[test]
fn merge_rejects_non_http_scheme() {
    let mut user = HashMap::new();
    user.insert("work".to_string(), "ftp://npm.work.example.com/".to_string());
    let err = merge_named_registries(&user).expect_err("ftp scheme must error");
    let MergeNamedRegistriesError::InvalidUrl { alias, url } = err else {
        panic!("expected InvalidUrl, got {err:?}");
    };
    assert_eq!(alias, "work");
    assert_eq!(url, "ftp://npm.work.example.com/");
}

/// The trailing slash on every built prefix is what makes
/// `https://npm.pkg.github.com-evil/` reject correctly.
#[test]
fn tarball_under_unrelated_prefix_does_not_match() {
    let prefixes = build_named_registry_prefixes(&HashMap::new());
    let regs = registries(&[("default", "https://registry.npmjs.org/")]);
    let picked = pick_registry_for_version(
        &regs,
        &prefixes,
        "foo",
        Some("https://npm.pkg.github.com-evil/foo-1.0.0.tgz"),
    );
    assert_eq!(picked, "https://registry.npmjs.org/");
}

#[test]
fn merge_rejects_reserved_alias() {
    let mut user = HashMap::new();
    user.insert("file".to_string(), "https://npm.work.example.com/".to_string());
    let err = merge_named_registries(&user).expect_err("reserved alias must error");
    assert!(matches!(err, MergeNamedRegistriesError::ReservedAlias { .. }), "got {err:?}");
}

#[test]
fn merge_rejects_malformed_alias() {
    let mut user = HashMap::new();
    user.insert("bad alias!".to_string(), "https://npm.work.example.com/".to_string());
    let err = merge_named_registries(&user).expect_err("malformed alias must error");
    assert!(matches!(err, MergeNamedRegistriesError::MalformedAlias { .. }), "got {err:?}");
}
