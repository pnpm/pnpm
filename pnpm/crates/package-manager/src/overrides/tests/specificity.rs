use super::{VersionsOverrider, dep_spec, manifest_from_value, parsed, undeclared};
use serde_json::json;
use std::path::Path;

fn apply_single_dep(overrides: &[(&str, &str)], dep_name: &str, declared: &str) -> String {
    let overrider = VersionsOverrider::new(&parsed(overrides), Path::new("/workspace"));
    let mut manifest = manifest_from_value(json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { dep_name: declared },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));
    dep_spec(&manifest, "dependencies", dep_name).expect("dependency remains present").to_string()
}

fn apply_with_parent(
    overrides: &[(&str, &str)],
    parent_name: &str,
    parent_version: &str,
    dep_name: &str,
    declared: &str,
) -> String {
    let overrider = VersionsOverrider::new(&parsed(overrides), Path::new("/workspace"));
    let mut manifest = manifest_from_value(json!({
        "name": parent_name,
        "version": parent_version,
        "dependencies": { dep_name: declared },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));
    dep_spec(&manifest, "dependencies", dep_name).expect("dependency remains present").to_string()
}

#[test]
fn exact_target_range_beats_caret_range_in_both_declaration_orders() {
    let broad_first =
        &[("my-lib@^1.0.0", "file:./lib-broad"), ("my-lib@1.0.0", "file:./lib-exact")];
    let exact_first =
        &[("my-lib@1.0.0", "file:./lib-exact"), ("my-lib@^1.0.0", "file:./lib-broad")];

    assert_eq!(apply_single_dep(broad_first, "my-lib", "1.0.0"), "file:lib-exact");
    assert_eq!(apply_single_dep(exact_first, "my-lib", "1.0.0"), "file:lib-exact");
    assert_eq!(apply_single_dep(broad_first, "my-lib", "1.2.0"), "file:lib-broad");
    assert_eq!(apply_single_dep(exact_first, "my-lib", "1.2.0"), "file:lib-broad");
}

#[test]
fn tilde_target_range_beats_caret_range_in_both_declaration_orders() {
    let caret_first = &[("my-lib@^1.0.0", "1.9.0"), ("my-lib@~1.2.0", "1.2.5")];
    let tilde_first = &[("my-lib@~1.2.0", "1.2.5"), ("my-lib@^1.0.0", "1.9.0")];

    assert_eq!(apply_single_dep(caret_first, "my-lib", "1.2.3"), "1.2.5");
    assert_eq!(apply_single_dep(tilde_first, "my-lib", "1.2.3"), "1.2.5");
}

#[test]
fn versioned_target_beats_unversioned_and_star_in_both_declaration_orders() {
    let unversioned_first = &[("foo", "3.0.0"), ("foo@*", "3.5.0"), ("foo@^3.0.0", "4.0.0")];
    let versioned_first = &[("foo@^3.0.0", "4.0.0"), ("foo@*", "3.5.0"), ("foo", "3.0.0")];

    assert_eq!(apply_single_dep(unversioned_first, "foo", "^3.0.0"), "4.0.0");
    assert_eq!(apply_single_dep(versioned_first, "foo", "^3.0.0"), "4.0.0");
    assert_eq!(apply_single_dep(unversioned_first, "foo", "^2.0.0"), "3.5.0");
    assert_eq!(apply_single_dep(versioned_first, "foo", "^2.0.0"), "3.5.0");
    assert_eq!(apply_single_dep(unversioned_first, "foo", "github:org/foo"), "3.0.0");
    assert_eq!(apply_single_dep(versioned_first, "foo", "github:org/foo"), "3.0.0");
}

#[test]
fn single_branch_range_beats_enclosing_union_range_in_both_orders() {
    let union_first = &[("foo@^1.0.0 || ^2.0.0", "2.5.0"), ("foo@^1.0.0", "1.5.0")];
    let single_first = &[("foo@^1.0.0", "1.5.0"), ("foo@^1.0.0 || ^2.0.0", "2.5.0")];

    assert_eq!(apply_single_dep(union_first, "foo", "^1.2.0"), "1.5.0");
    assert_eq!(apply_single_dep(single_first, "foo", "^1.2.0"), "1.5.0");
    assert_eq!(apply_single_dep(union_first, "foo", "^2.1.0"), "2.5.0");
    assert_eq!(apply_single_dep(single_first, "foo", "^2.1.0"), "2.5.0");
}

#[test]
fn narrower_parent_range_beats_broader_parent_range_in_both_orders() {
    let broad_first =
        &[("parent>dep", "1.0.0"), ("parent@^1.0.0>dep", "2.0.0"), ("parent@1.2.0>dep", "3.0.0")];
    let narrow_first =
        &[("parent@1.2.0>dep", "3.0.0"), ("parent@^1.0.0>dep", "2.0.0"), ("parent>dep", "1.0.0")];

    assert_eq!(apply_with_parent(broad_first, "parent", "1.2.0", "dep", "^1.0.0"), "3.0.0");
    assert_eq!(apply_with_parent(narrow_first, "parent", "1.2.0", "dep", "^1.0.0"), "3.0.0");
    assert_eq!(apply_with_parent(broad_first, "parent", "1.5.0", "dep", "^1.0.0"), "2.0.0");
    assert_eq!(apply_with_parent(narrow_first, "parent", "1.5.0", "dep", "^1.0.0"), "2.0.0");
    assert_eq!(apply_with_parent(broad_first, "parent", "2.0.0", "dep", "^1.0.0"), "1.0.0");
    assert_eq!(apply_with_parent(narrow_first, "parent", "2.0.0", "dep", "^1.0.0"), "1.0.0");
}

#[test]
fn incomparable_overlapping_ranges_preserve_declaration_order_and_never_revive_dominated_entry() {
    let left_first = &[("foo@>=1.0.0 <2.0.0", "1.8.0"), ("foo@>=1.5.0 <3.0.0", "2.8.0")];
    let right_first = &[("foo@>=1.5.0 <3.0.0", "2.8.0"), ("foo@>=1.0.0 <2.0.0", "1.8.0")];

    assert_eq!(apply_single_dep(left_first, "foo", "1.6.0"), "1.8.0");
    assert_eq!(apply_single_dep(right_first, "foo", "1.6.0"), "2.8.0");

    // `>=1.0.0 <2.0.0` is strictly dominated by `>=1.0.0 <1.5.0`, even with an
    // incomparable range between them.
    let chain = &[
        ("foo@>=1.0.0 <2.0.0", "broad"),
        ("foo@>=1.2.0 <2.5.0", "incomparable"),
        ("foo@>=1.0.0 <1.5.0", "narrow"),
    ];
    assert_eq!(apply_single_dep(chain, "foo", "1.3.0"), "incomparable");
}

#[test]
fn prerelease_specificity_respects_tuple_eligibility() {
    let broad_first = &[("foo@^1.0.0-alpha.0", "1.9.0"), ("foo@1.0.0-alpha.1", "1.0.0-alpha.2")];
    let exact_first = &[("foo@1.0.0-alpha.1", "1.0.0-alpha.2"), ("foo@^1.0.0-alpha.0", "1.9.0")];

    assert_eq!(apply_single_dep(broad_first, "foo", "1.0.0-alpha.1"), "1.0.0-alpha.2");
    assert_eq!(apply_single_dep(exact_first, "foo", "1.0.0-alpha.1"), "1.0.0-alpha.2");
}

#[test]
fn undeclared_dependency_override_uses_narrower_range_in_both_orders() {
    let broad_first = VersionsOverrider::new(
        &parsed(&[("foo@^1.0.0", "1.9.0"), ("foo@1.0.0", "1.0.1")]),
        Path::new("/workspace"),
    );
    let exact_first = VersionsOverrider::new(
        &parsed(&[("foo@1.0.0", "1.0.1"), ("foo@^1.0.0", "1.9.0")]),
        Path::new("/workspace"),
    );

    assert_eq!(undeclared(&broad_first, "foo", "1.0.0").as_deref(), Some("1.0.1"));
    assert_eq!(undeclared(&exact_first, "foo", "1.0.0").as_deref(), Some("1.0.1"));
}
