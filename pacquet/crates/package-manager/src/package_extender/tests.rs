use super::PackageExtender;
use indexmap::IndexMap;
use pacquet_config::{PackageExtension, PeerDependencyMeta};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

fn extension(deps: &[(&str, &str)]) -> PackageExtension {
    let mut dependencies = BTreeMap::new();
    for (name, range) in deps {
        dependencies.insert((*name).to_string(), (*range).to_string());
    }
    PackageExtension { dependencies: Some(dependencies), ..Default::default() }
}

#[test]
fn extends_manifest_with_missing_dependency() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive".to_string(), extension(&[("@pnpm.e2e/bar", "100.1.0")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let mut manifest = json!({
        "name": "is-positive",
        "version": "1.0.0",
    });
    extender.apply(&mut manifest);

    assert_eq!(
        manifest,
        json!({
            "name": "is-positive",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/bar": "100.1.0" },
        }),
    );
}

#[test]
fn manifest_own_dep_wins_over_extension() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive".to_string(), extension(&[("@pnpm.e2e/bar", "100.1.0")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let mut manifest = json!({
        "name": "is-positive",
        "version": "1.0.0",
        "dependencies": { "@pnpm.e2e/bar": "200.0.0" },
    });
    extender.apply(&mut manifest);

    assert_eq!(
        manifest.get("dependencies").unwrap(),
        &json!({ "@pnpm.e2e/bar": "200.0.0" }),
        "manifest's own dep range must override the extension's",
    );
}

#[test]
fn range_filter_applies_only_to_matching_versions() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive@^1.0.0".to_string(), extension(&[("a", "1")]));
    extensions.insert("is-positive@^2.0.0".to_string(), extension(&[("b", "1")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let mut v1 = json!({ "name": "is-positive", "version": "1.0.5" });
    extender.apply(&mut v1);
    assert_eq!(v1.get("dependencies").unwrap(), &json!({ "a": "1" }));

    let mut v2 = json!({ "name": "is-positive", "version": "2.3.4" });
    extender.apply(&mut v2);
    assert_eq!(v2.get("dependencies").unwrap(), &json!({ "b": "1" }));
}

#[test]
fn unrelated_package_unchanged() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive".to_string(), extension(&[("@pnpm.e2e/bar", "100.1.0")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let mut manifest = json!({ "name": "is-negative", "version": "1.0.0" });
    let before = manifest.clone();
    extender.apply(&mut manifest);
    assert_eq!(manifest, before);
}

#[test]
fn extends_peer_dependencies_and_meta() {
    let mut meta = BTreeMap::new();
    meta.insert("react".to_string(), PeerDependencyMeta { optional: Some(true) });
    let mut peers = BTreeMap::new();
    peers.insert("react".to_string(), ">=16".to_string());
    let ext = PackageExtension {
        peer_dependencies: Some(peers),
        peer_dependencies_meta: Some(meta),
        ..Default::default()
    };
    let mut extensions = IndexMap::new();
    extensions.insert("some-pkg".to_string(), ext);
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let mut manifest = json!({ "name": "some-pkg", "version": "1.0.0" });
    extender.apply(&mut manifest);

    assert_eq!(manifest.get("peerDependencies").unwrap(), &json!({ "react": ">=16" }));
    assert_eq!(
        manifest.get("peerDependenciesMeta").unwrap(),
        &json!({ "react": { "optional": true } }),
    );
}

#[test]
fn apply_to_arc_returns_same_arc_when_no_match() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive".to_string(), extension(&[("a", "1")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let manifest = Arc::new(json!({ "name": "is-negative", "version": "1.0.0" }));
    let before_ptr = Arc::as_ptr(&manifest);
    let after = extender.apply_to_arc(manifest);
    assert_eq!(before_ptr, Arc::as_ptr(&after), "unchanged manifests must keep the same Arc");
}

#[test]
fn unparsable_range_in_selector_returns_construction_error() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive@~~garbage".to_string(), extension(&[("dep", "1")]));
    let err =
        PackageExtender::new(&extensions).expect_err("malformed range must surface as an error");
    assert_eq!(err.selector, "is-positive@~~garbage");
    assert_eq!(err.range, "~~garbage");
}

#[test]
fn apply_to_arc_returns_same_arc_when_only_range_mismatches() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive@^1.0.0".to_string(), extension(&[("a", "1")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let manifest = Arc::new(json!({ "name": "is-positive", "version": "2.0.0" }));
    let before_ptr = Arc::as_ptr(&manifest);
    let after = extender.apply_to_arc(manifest);
    assert_eq!(
        before_ptr,
        Arc::as_ptr(&after),
        "non-matching range must not allocate a fresh manifest",
    );
}

#[test]
fn apply_to_arc_clones_on_match() {
    let mut extensions = IndexMap::new();
    extensions.insert("is-positive".to_string(), extension(&[("a", "1")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let manifest = Arc::new(json!({ "name": "is-positive", "version": "1.0.0" }));
    let before_ptr = Arc::as_ptr(&manifest);
    let after = extender.apply_to_arc(manifest);
    assert_ne!(before_ptr, Arc::as_ptr(&after), "matched manifests must be cloned");
    assert_eq!(after.get("dependencies").unwrap(), &json!({ "a": "1" }));
}

#[test]
fn empty_extender_is_noop() {
    let extender = PackageExtender::new(&IndexMap::new()).expect("empty map produces no errors");
    assert!(extender.is_empty());
    let manifest = Arc::new(json!({ "name": "x", "version": "1.0.0" }));
    let before_ptr = Arc::as_ptr(&manifest);
    let after = extender.apply_to_arc(manifest);
    assert_eq!(before_ptr, Arc::as_ptr(&after));
}

#[test]
fn scoped_package_selector_is_split_correctly() {
    let mut extensions = IndexMap::new();
    extensions.insert("@scope/foo@^1".to_string(), extension(&[("dep", "1")]));
    let extender = PackageExtender::new(&extensions).expect("valid selectors in test fixtures");

    let mut manifest = json!({ "name": "@scope/foo", "version": "1.5.0" });
    extender.apply(&mut manifest);
    assert_eq!(manifest.get("dependencies").unwrap(), &json!({ "dep": "1" }));
}

#[test]
fn checksum_order_invariance_outer_keys() {
    let pair_a = (
        "is-odd".to_string(),
        PackageExtension {
            peer_dependencies: Some({
                let mut extensions = BTreeMap::new();
                extensions.insert("is-number".to_string(), "*".to_string());
                extensions
            }),
            ..Default::default()
        },
    );
    let pair_b = (
        "is-even".to_string(),
        PackageExtension {
            peer_dependencies: Some({
                let mut extensions = BTreeMap::new();
                extensions.insert("is-number".to_string(), "*".to_string());
                extensions
            }),
            ..Default::default()
        },
    );
    let mut ext1 = IndexMap::new();
    ext1.insert(pair_a.0.clone(), pair_a.1.clone());
    ext1.insert(pair_b.0.clone(), pair_b.1.clone());
    let mut ext2 = IndexMap::new();
    ext2.insert(pair_b.0, pair_b.1);
    ext2.insert(pair_a.0, pair_a.1);

    let extender1 = PackageExtender::new(&ext1).expect("valid selectors in test fixtures");
    let extender2 = PackageExtender::new(&ext2).expect("valid selectors in test fixtures");

    let mut m1 = json!({ "name": "is-odd", "version": "1.0.0" });
    let mut m2: Value = m1.clone();
    extender1.apply(&mut m1);
    extender2.apply(&mut m2);
    assert_eq!(m1, m2);
}
