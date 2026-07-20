use super::{
    ImporterUpdateSeedPolicy, UpdateSeedPolicy, compute_package_extensions_checksum,
    full_resolution_required, is_partial_workspace_selection, update_reuse_scopes,
};
use pacquet_config::{Config, PackageExtension};
use pretty_assertions::assert_eq;

fn config_with_extensions(entries: &[(&str, &[(&str, &str)])]) -> Box<Config> {
    let mut extensions = indexmap::IndexMap::new();
    for (selector, deps) in entries {
        let mut dependencies = std::collections::BTreeMap::new();
        for (name, range) in *deps {
            dependencies.insert((*name).to_string(), (*range).to_string());
        }
        extensions.insert(
            (*selector).to_string(),
            PackageExtension { dependencies: Some(dependencies), ..Default::default() },
        );
    }
    let mut config = Config::new();
    config.package_extensions = Some(extensions);
    Box::new(config)
}

#[test]
fn full_workspace_selection_keeps_resolution_prefetch_enabled() {
    let real = std::collections::HashSet::from(["a".to_string(), "b".to_string()]);
    let all_selected = real.clone();
    let partial = std::collections::HashSet::from(["a".to_string()]);

    assert!(!is_partial_workspace_selection(Some(&real), Some(&all_selected)));
    assert!(is_partial_workspace_selection(Some(&real), Some(&partial)));
    assert!(!is_partial_workspace_selection(None, None));
}

/// Ports `installing/.../packageExtensions.ts:103-153`
/// `packageExtensionsChecksum does not change regardless of keys
/// order` — two `Config::package_extensions` populated with the
/// same selectors and entries in a different declared order must
/// produce the same `sha256-…` lockfile checksum. Without the
/// sorted-keys hash, the order-sensitive `IndexMap` iteration
/// would flap the checksum and force a redundant full resolution
/// on every reorder.
#[test]
fn compute_checksum_is_order_invariant_across_outer_keys() {
    let config_a = config_with_extensions(&[
        ("is-odd", &[("is-number", "*")]),
        ("is-even", &[("is-number", "*")]),
    ]);
    let config_b = config_with_extensions(&[
        ("is-even", &[("is-number", "*")]),
        ("is-odd", &[("is-number", "*")]),
    ]);
    let checksum_a = compute_package_extensions_checksum(&config_a);
    let checksum_b = compute_package_extensions_checksum(&config_b);
    assert!(checksum_a.is_some(), "configured extensions must hash to Some");
    assert_eq!(checksum_a, checksum_b);
}

/// Empty / absent extensions round-trip to `None`, matching pnpm's
/// `hashObjectNullableWithPrefix(undefined) === undefined`
/// short-circuit. Without this, an absent `packageExtensions` and
/// a configured-but-empty one would write different lockfile
/// fields and the drift gate would fire on no-op installs.
#[test]
fn compute_checksum_is_none_when_extensions_absent() {
    let config = Config::new();
    assert_eq!(compute_package_extensions_checksum(&config), None);
}

/// `Some({})` (an explicitly empty map) also collapses to `None`,
/// mirroring pnpm's `if (!object || isEmpty(object)) return undefined`.
/// Without the empty-map guard, an explicit `packageExtensions: {}`
/// in pnpm-workspace.yaml — or an env-var-driven override clearing
/// a parent layer — would hash to a checksum while pnpm omits the
/// field, causing spurious drift on cross-tool installs.
#[test]
fn compute_checksum_is_none_for_explicit_empty_map() {
    let mut config = Config::new();
    config.package_extensions = Some(indexmap::IndexMap::new());
    assert_eq!(compute_package_extensions_checksum(&config), None);
}

#[test]
fn importer_scoped_update_full_resolution_requires_every_importer_to_disable_reuse() {
    use pacquet_resolving_deps_resolver::UpdateReuseScope;

    let importer_ids = ["selected", "unselected"];
    let mixed =
        std::collections::BTreeMap::from([("selected".to_string(), UpdateReuseScope::None)]);
    assert!(!full_resolution_required(true, importer_ids, &UpdateReuseScope::All, &mixed,));

    let all_none = std::collections::BTreeMap::from([
        ("selected".to_string(), UpdateReuseScope::None),
        ("unselected".to_string(), UpdateReuseScope::None),
    ]);
    assert!(full_resolution_required(true, importer_ids, &UpdateReuseScope::All, &all_none,));
    assert!(full_resolution_required(
        false,
        importer_ids,
        &UpdateReuseScope::All,
        &std::collections::BTreeMap::new(),
    ));
}

#[test]
fn importer_scoped_update_custom_refresh_widens_every_importer() {
    use pacquet_resolving_deps_resolver::UpdateReuseScope;

    let scoped = std::collections::BTreeMap::from([(
        "selected".to_string(),
        UpdateReuseScope::Except(std::collections::HashSet::from(["pkg".to_string()])),
    )]);
    assert!(full_resolution_required(
        true,
        ["selected", "unselected"],
        &UpdateReuseScope::None,
        &scoped,
    ));
}

#[test]
fn importer_scoped_update_absent_importer_keeps_all_reuse() {
    use pacquet_resolving_deps_resolver::UpdateReuseScope;

    let policy = UpdateSeedPolicy::ByImporter(std::collections::BTreeMap::from([(
        "selected".to_string(),
        ImporterUpdateSeedPolicy::DropAll,
    )]));
    let (default_scope, scopes) = update_reuse_scopes(&policy);
    assert_eq!(default_scope, UpdateReuseScope::All);
    assert_eq!(scopes.get("selected"), Some(&UpdateReuseScope::None));
    assert!(!scopes.contains_key("unselected"));
}
