use std::collections::HashMap;

use pretty_assertions::assert_eq;

use super::{
    MergeNamedRegistriesError, build_named_registry_prefixes, merge_named_registries,
    pick_registry_for_package, pick_registry_for_version,
};

fn registries(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}

/// The `gh:` builtin always lands in the prefix list, even when no
/// user-supplied named registries are configured.
#[test]
fn build_prefixes_includes_gh_builtin() {
    let prefixes = build_named_registry_prefixes(&HashMap::new());
    assert!(prefixes.iter().any(|prefix| prefix == "https://npm.pkg.github.com/"));
}

/// User-supplied named registries override the builtins on key
/// collision (later wins, matches upstream's spread semantics).
#[test]
fn build_prefixes_overrides_builtin_on_same_key() {
    let mut named = HashMap::new();
    named.insert("gh".to_string(), "https://internal/gh/".to_string());
    let prefixes = build_named_registry_prefixes(&named);
    assert!(prefixes.iter().any(|prefix| prefix == "https://internal/gh/"));
    assert!(!prefixes.iter().any(|prefix| prefix == "https://npm.pkg.github.com/"));
}

/// Two registries sharing a host but different paths each get a
/// trailing-slash prefix; the longest match comes first so the
/// caller picks the deeper one.
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

/// When the lockfile records a tarball URL falling under a named
/// registry, `pick_registry_for_version` returns the prefix even if
/// scope routing would have picked the default.
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

/// Without a tarball URL, routing falls through to scope-vs-default.
/// A scoped name with a matching `registries[@scope]` entry wins; an
/// unscoped name hits `registries["default"]`.
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

/// `pick_registry_for_package` consults the `npm:@scope/...` form of
/// `bare_specifier` for the scope override before falling back to the
/// scope of the local package name. Without this, an npm-alias entry
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

/// When no `npm:` prefix is in play, the local package name's scope
/// still drives routing (preserves the legacy behavior for plain
/// `"@scope/foo": "^1"` manifest entries).
#[test]
fn falls_back_to_pkg_name_scope_without_npm_alias() {
    let regs = registries(&[
        ("default", "https://registry.npmjs.org/"),
        ("@private", "https://internal/registry/"),
    ]);
    let picked = pick_registry_for_package(&regs, "@private/foo", Some("^1.0.0"));
    assert_eq!(picked, "https://internal/registry/");
}

/// Scoped local name + scoped `npm:` target in a **different scope**:
/// the target's scope wins. The package being fetched is
/// `@scope2/bar`, so routing follows `@scope2`, not the local
/// `@scope1/` slot. Mirrors upstream's `'npm:@private/lodash@1'`
/// case in `config/pick-registry-for-package/test/index.spec.ts`.
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

/// An unscoped `npm:` alias target (`"@private/foo": "npm:lodash@^1"`)
/// routes through the **default** registry, not the local alias's
/// scope. The fetched package is `lodash` (unscoped); the local
/// `@private/` slot is just where the install lands in
/// `node_modules`, and `lodash` doesn't live on a scoped registry.
/// Mirrors the upstream fix in
/// `config/pick-registry-for-package`.
#[test]
fn unscoped_npm_alias_target_routes_to_default() {
    let regs = registries(&[
        ("default", "https://registry.npmjs.org/"),
        ("@private", "https://internal/registry/"),
    ]);
    let picked = pick_registry_for_package(&regs, "@private/foo", Some("npm:lodash@^1"));
    assert_eq!(picked, "https://registry.npmjs.org/");
}

/// The merged map always carries pnpm's built-in `gh:` alias even
/// when the user supplies no entries — mirrors upstream's
/// `{ ...BUILTIN_NAMED_REGISTRIES }` spread.
#[test]
fn merge_includes_builtin_when_user_empty() {
    let merged = merge_named_registries(&HashMap::new()).unwrap();
    assert_eq!(merged.get("gh").map(String::as_str), Some("https://npm.pkg.github.com/"));
}

/// User-defined aliases override the built-in `gh:` on key collision
/// — GHES users redirect the alias at their enterprise host.
#[test]
fn merge_user_overrides_builtin_gh() {
    let mut user = HashMap::new();
    user.insert("gh".to_string(), "https://npm.ghes.example.com/".to_string());
    let merged = merge_named_registries(&user).unwrap();
    assert_eq!(merged.get("gh").map(String::as_str), Some("https://npm.ghes.example.com/"));
}

/// Malformed URLs surface at construction with
/// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL` — a missing scheme is
/// caught now rather than as a 404 during resolution.
#[test]
fn merge_rejects_url_without_scheme() {
    let mut user = HashMap::new();
    user.insert("work".to_string(), "npm.work.example.com".to_string());
    let err = merge_named_registries(&user).expect_err("missing scheme must error");
    assert!(matches!(err, MergeNamedRegistriesError::InvalidUrl { .. }), "got {err:?}");
}

/// Non-http(s) schemes are rejected — `ftp://`, `file://`, etc. are
/// not valid registry transports.
#[test]
fn merge_rejects_non_http_scheme() {
    let mut user = HashMap::new();
    user.insert("work".to_string(), "ftp://npm.work.example.com/".to_string());
    let err = merge_named_registries(&user).expect_err("ftp scheme must error");
    let MergeNamedRegistriesError::InvalidUrl { alias, url } = err;
    assert_eq!(alias, "work");
    assert_eq!(url, "ftp://npm.work.example.com/");
}

/// A tarball URL that's *almost* a prefix match — same host, but
/// without the trailing slash on the prefix — must not silently
/// route. The trailing-slash on every built prefix is what makes
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
