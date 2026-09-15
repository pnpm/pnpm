use super::{
    Lockfile, StalenessReason, manifest_from_json, satisfies_package_manifest, text_block,
};

#[test]
fn matching_across_all_three_dep_fields_satisfies() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
        "    devDependencies:"
        "      typescript:"
        "        specifier: ^5.0.0"
        "        version: 5.1.6"
        "    optionalDependencies:"
        "      fsevents:"
        "        specifier: ^2.0.0"
        "        version: 2.3.3"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "react": "^17.0.2" },
        "devDependencies": { "typescript": "^5.0.0" },
        "optionalDependencies": { "fsevents": "^2.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

/// Same name + specifier moved between fields (`devDependencies` →
/// `dependencies`) should be caught by the per-field follow-up loop.
/// The flat-record pre-pass would say "specifiers match" because
/// they do across the union — but the dep-graph install would be
/// different so we must reject.
#[test]
fn dep_moves_between_fields_returns_dep_specifier_mismatch() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    devDependencies:"
        "      typescript:"
        "        specifier: ^5.0.0"
        "        version: 5.1.6"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "typescript": "^5.0.0" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::DepSpecifierMismatch { .. }),
        "expected DepSpecifierMismatch, got {err:?}",
    );
}

/// `SpecDiff::Display` produces stable, user-readable output. Pins
/// the wording roughly: a regression in the format string would
/// silently scramble the error message users see in CI logs.
#[test]
fn spec_diff_display_lists_added_removed_modified() {
    let mut diff = super::super::SpecDiff::default();
    diff.added.insert("lodash".to_string(), "^4.0.0".to_string());
    diff.added.insert("ramda".to_string(), "^0.30.0".to_string());
    diff.removed.insert("underscore".to_string(), "^1.0.0".to_string());
    diff.modified.insert("react".to_string(), ("^17.0.2".to_string(), "^18.0.0".to_string()));
    let rendered = diff.to_string();
    assert!(rendered.contains("2 dependencies were added: "));
    assert!(rendered.contains("1 dependency was removed: underscore@^1.0.0"));
    assert!(rendered.contains("1 dependency is mismatched:"));
    assert!(rendered.contains("react (lockfile: ^17.0.2, manifest: ^18.0.0)"));
}

#[test]
fn cross_field_swap_with_same_cardinalities_caught_by_per_field_check() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
        "    devDependencies:"
        "      typescript:"
        "        specifier: ^5.0.0"
        "        version: 5.1.6"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "typescript": "^5.0.0" },
        "devDependencies": { "react": "^17.0.2" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::DepSpecifierMismatch { .. }),
        "expected DepSpecifierMismatch for cross-field swap, got {err:?}",
    );
}

#[test]
fn spec_diff_display_lists_plural_removed_and_modified_with_separators() {
    let mut diff = super::super::SpecDiff::default();
    diff.removed.insert("alpha".to_string(), "^1.0.0".to_string());
    diff.removed.insert("beta".to_string(), "^2.0.0".to_string());
    diff.modified.insert("gamma".to_string(), ("^3.0.0".to_string(), "^4.0.0".to_string()));
    diff.modified.insert("delta".to_string(), ("^0.1.0".to_string(), "^0.2.0".to_string()));
    let rendered = diff.to_string();
    assert!(rendered.contains("2 dependencies were removed: "), "got: {rendered:?}");
    assert!(
        rendered.contains("alpha@^1.0.0, beta@^2.0.0")
            || rendered.contains("beta@^2.0.0, alpha@^1.0.0"),
        "expected comma-joined removed entries, got: {rendered:?}",
    );
    assert!(rendered.contains("2 dependencies are mismatched:"), "got: {rendered:?}");
}

#[test]
fn spec_diff_display_uses_singular_for_count_of_one() {
    let mut diff = super::super::SpecDiff::default();
    diff.added.insert("foo".to_string(), "^1.0.0".to_string());
    let rendered = diff.to_string();
    assert!(
        rendered.contains("1 dependency was added: "),
        "expected singular wording for count of 1, got: {rendered:?}",
    );
    assert!(!rendered.contains("dependencies were added"));
}

#[test]
fn same_dep_in_prod_and_dev_counts_under_prod() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      foo:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "1.0.0" },
        "devDependencies": { "foo": "1.0.0" }
    }"#,
    );
    assert!(
        satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok(),
        "manifest listing foo in prod+dev must satisfy a lockfile that records it under prod only",
    );
}
