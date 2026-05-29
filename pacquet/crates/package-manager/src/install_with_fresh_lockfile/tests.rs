use super::compute_package_extensions_checksum;
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
