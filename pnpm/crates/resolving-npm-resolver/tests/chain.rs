//! Chain-integration tests for [`NamedRegistryResolver`] alias
//! validation.
//!
//! A named-registry alias that would shadow an explicit local-scheme
//! protocol (`link:` / `workspace:` / `file:`) or any other reserved
//! specifier prefix is rejected at resolver construction — since the
//! lockfile 12.0 format writes `<name>@<alias>:<version>` keys, such an
//! alias would make the version slot ambiguous.

use std::collections::HashMap;

use pnpm_resolving_npm_resolver::{MergeNamedRegistriesError, merge_named_registries};

/// Reserved local-scheme aliases are rejected up front instead of being
/// silently shadowed by the local resolvers in the chain.
#[test]
fn reserved_scheme_aliases_are_rejected() {
    for alias in ["link", "workspace", "file", "runtime", "pkg", "PKG", "Pkg"] {
        let mut user = HashMap::new();
        user.insert(alias.to_string(), "https://npm.work.example.com/".to_string());
        let err = merge_named_registries(&user).expect_err("reserved alias must error");
        assert!(
            matches!(err, MergeNamedRegistriesError::ReservedAlias { .. }),
            "alias {alias}: got {err:?}",
        );
    }
}

/// Only a prefix a selector may spell in any case is reserved that way: a
/// `npm:` specifier is read exactly, so `Npm` stays usable as an alias.
#[test]
fn a_mixed_case_alias_is_reserved_only_where_its_prefix_is() {
    let mut user = HashMap::new();
    user.insert("Npm".to_string(), "https://npm.work.example.com/".to_string());

    let merged = merge_named_registries(&user).expect("Npm does not shadow a reserved prefix");

    assert_eq!(merged.get("Npm").map(String::as_str), Some("https://npm.work.example.com/"));
}
