use super::{StalenessReason, check_lockfile_settings, satisfies_package_manifest};
use crate::Lockfile;
use pacquet_package_manifest::PackageManifest;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use text_block_macros::text_block;

/// Build a `PackageManifest` from inline JSON. Writes it to a temp
/// file so [`PackageManifest::from_path`] can parse it back through
/// the normal load path — exercises the actual deserialize, not a
/// `serde_json::Value` shortcut.
fn manifest_from_json(json: &str) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("package.json");
    std::fs::write(&path, json).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

/// Single-importer lockfile + matching manifest passes the check.
/// Baseline for every test below — if this fails everything else is
/// noise.
#[test]
fn matching_manifest_and_lockfile_satisfies() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": {
            "react": "^17.0.2"
        }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false).is_ok());
}

/// Manifest lists a dep the lockfile doesn't. Should surface as
/// `SpecifiersDiffer` with the missing entry in `added`.
#[test]
fn manifest_adds_dep_returns_specifier_diff() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": {
            "react": "^17.0.2",
            "lodash": "^4.17.21"
        }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
        .expect_err("should be stale");
    let StalenessReason::SpecifiersDiffer(diff) = err else {
        panic!("expected SpecifiersDiffer, got {err:?}");
    };
    assert_eq!(diff.added.get("lodash").map(String::as_str), Some("^4.17.21"));
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
}

/// Lockfile lists a dep the manifest dropped. Should surface as a
/// `SpecifiersDiffer` with the dropped entry in `removed`.
#[test]
fn manifest_drops_dep_returns_specifier_diff() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
        "      lodash:"
        "        specifier: ^4.17.21"
        "        version: 4.17.21"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": {
            "react": "^17.0.2"
        }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
        .expect_err("should be stale");
    let StalenessReason::SpecifiersDiffer(diff) = err else {
        panic!("expected SpecifiersDiffer, got {err:?}");
    };
    assert_eq!(diff.removed.get("lodash").map(String::as_str), Some("^4.17.21"));
}

/// Same dep, same name, different specifier. Should surface as a
/// `SpecifiersDiffer` with the (lockfile, manifest) pair in
/// `modified`. This is the "user bumped a dep in package.json
/// without re-running install" case — the most common drift cause.
#[test]
fn manifest_bumps_specifier_returns_specifier_diff() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": {
            "react": "^18.0.0"
        }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
        .expect_err("should be stale");
    let StalenessReason::SpecifiersDiffer(diff) = err else {
        panic!("expected SpecifiersDiffer, got {err:?}");
    };
    let modified = diff.modified.get("react").expect("react bucketed under modified");
    assert_eq!(modified.0, "^17.0.2");
    assert_eq!(modified.1, "^18.0.0");
}

/// Manifest with dev + optional in addition to prod, all matching
/// the lockfile. Confirms the flat-union pre-pass treats all three
/// fields equally.
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
    assert!(satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false).is_ok());
}

/// Lockfile has no `importers["."]` entry — even though pacquet's
/// `Lockfile` type makes `importers` a map (so an empty map is a
/// valid shape), we still want to fail cleanly when the importer the
/// caller asked about isn't present.
#[test]
fn missing_importer_returns_no_importer() {
    // Build a manually-constructed lockfile with empty importers.
    let lockfile: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse minimal lockfile");
    // We can't easily get a `ProjectSnapshot` out of an empty map,
    // so this test exercises the lookup-then-call shape on the
    // caller side: the caller uses `root_project()` which returns
    // `None`, and the `NoImporter` reason is constructed there.
    assert!(lockfile.root_project().is_none());
}

/// Same name + specifier moved between fields (`devDependencies` →
/// `dependencies`) should be caught by the per-field follow-up loop.
/// The flat-record pre-pass would say "specifiers match" because
/// they do across the union — but the dep-graph install would be
/// different so we must reject. Mirrors upstream's per-field check
/// at <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L67-L100>.
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
    // Same name + specifier, but now in `dependencies` instead of
    // `devDependencies`.
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "typescript": "^5.0.0" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
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
    let mut diff = super::SpecDiff::default();
    diff.added.insert("lodash".to_string(), "^4.0.0".to_string());
    diff.added.insert("ramda".to_string(), "^0.30.0".to_string());
    diff.removed.insert("underscore".to_string(), "^1.0.0".to_string());
    diff.modified.insert("react".to_string(), ("^17.0.2".to_string(), "^18.0.0".to_string()));
    let rendered = diff.to_string();
    // Plural noun + plural verb for n>1.
    assert!(rendered.contains("2 dependencies were added: "));
    // Singular noun + singular verb for n==1 — the Copilot review
    // catch ("1 dependencies were added" was grammatically wrong).
    assert!(rendered.contains("1 dependency was removed: underscore@^1.0.0"));
    assert!(rendered.contains("1 dependency is mismatched:"));
    assert!(rendered.contains("react (lockfile: ^17.0.2, manifest: ^18.0.0)"));
}

/// Two deps swapped between fields with same cardinality on each
/// side: lockfile has `react` under `dependencies` + `typescript`
/// under `devDependencies`, manifest swaps them. The flat-union diff
/// over `(deps ∪ devDeps ∪ optDeps)` matches because the union is
/// identical, so the per-field check is the only thing that can
/// catch this. Pre-fix the per-field loop only ran when field
/// cardinalities differed; this test guards against that regression.
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
    // Same names + specifiers as the lockfile, but `react` and
    // `typescript` swap fields.
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "typescript": "^5.0.0" },
        "devDependencies": { "react": "^17.0.2" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::DepSpecifierMismatch { .. }),
        "expected DepSpecifierMismatch for cross-field swap, got {err:?}",
    );
}

/// `publishDirectory` on the lockfile differing from
/// `publishConfig.directory` on the manifest fails the check.
/// Mirrors upstream's `publishDirectory` mismatch.
#[test]
fn publish_directory_mismatch_returns_publish_directory_mismatch() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    publishDirectory: ./dist"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "publishConfig": { "directory": "./build" },
        "dependencies": { "react": "^17.0.2" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::PublishDirectoryMismatch { .. }),
        "expected PublishDirectoryMismatch, got {err:?}",
    );
}

/// `dependenciesMeta` mismatch (different `injected` flag) fails
/// the check. Two `None`s and `None`-vs-empty-object are both
/// considered equal — that's a separate happy-path case.
#[test]
fn dependencies_meta_mismatch_returns_dependencies_meta_mismatch() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      foo:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
        "    dependenciesMeta:"
        "      foo:"
        "        injected: true"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "^1.0.0" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::DependenciesMetaMismatch { .. }),
        "expected DependenciesMetaMismatch, got {err:?}",
    );
}

/// `NoImporter` message renders with `importers["."]`-style
/// formatting, not `importers."."` (the previous `{:?}` debug-
/// format output). Caught in Copilot review on [#450] — debug-format
/// quoting reads poorly for short keys like `.`.
///
/// [#450]: https://github.com/pnpm/pacquet/pull/450
#[test]
fn no_importer_message_uses_bracket_quoted_id() {
    let reason = StalenessReason::NoImporter { importer_id: ".".to_string() };
    let rendered = reason.to_string();
    assert!(rendered.contains(r#"importers["."]"#), "expected bracket-quoted id, got {rendered:?}");
    assert!(
        !rendered.contains(r#"importers.".""#),
        "must not use Rust debug-format quoting, got {rendered:?}",
    );
}

/// Multi-element `removed` and `modified` buckets exercise the
/// plural arm of `noun_verb_for` *and* the comma separator inside
/// the per-bucket loops. The single-item display test above only
/// hits the singular branch and the first-iteration of the loop.
#[test]
fn spec_diff_display_lists_plural_removed_and_modified_with_separators() {
    let mut diff = super::SpecDiff::default();
    diff.removed.insert("alpha".to_string(), "^1.0.0".to_string());
    diff.removed.insert("beta".to_string(), "^2.0.0".to_string());
    diff.modified.insert("gamma".to_string(), ("^3.0.0".to_string(), "^4.0.0".to_string()));
    diff.modified.insert("delta".to_string(), ("^0.1.0".to_string(), "^0.2.0".to_string()));
    let rendered = diff.to_string();
    // Plural noun + plural verb for n>1 (`were`).
    assert!(rendered.contains("2 dependencies were removed: "), "got: {rendered:?}");
    // Comma separator inside the removed loop.
    assert!(
        rendered.contains("alpha@^1.0.0, beta@^2.0.0")
            || rendered.contains("beta@^2.0.0, alpha@^1.0.0"),
        "expected comma-joined removed entries, got: {rendered:?}",
    );
    // Plural noun + plural verb for n>1 `modified` (`are`).
    assert!(rendered.contains("2 dependencies are mismatched:"), "got: {rendered:?}");
}

/// Pinpoint singular-vs-plural wording per bucket so the n==1 case
/// doesn't silently regress.
#[test]
fn spec_diff_display_uses_singular_for_count_of_one() {
    let mut diff = super::SpecDiff::default();
    diff.added.insert("foo".to_string(), "^1.0.0".to_string());
    let rendered = diff.to_string();
    assert!(
        rendered.contains("1 dependency was added: "),
        "expected singular wording for count of 1, got: {rendered:?}",
    );
    assert!(!rendered.contains("dependencies were added"));
}

/// `dependenciesMeta: {}` on the manifest with no `dependenciesMeta`
/// on the importer should match — empty object and absent are
/// equivalent. Mirrors upstream's `?? {}` coercion at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L56-L58>.
/// Ports the upstream test at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/test/satisfiesPackageManifest.ts#L232-L252>.
#[test]
fn dependencies_meta_empty_object_equivalent_to_absent() {
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
    // Manifest has `dependenciesMeta: {}`; lockfile has none.
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "1.0.0" },
        "dependenciesMeta": {}
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false).is_ok());
}

/// `publishDirectory` happy-path: lockfile and manifest agree on the
/// directory. Ports the upstream test at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/test/satisfiesPackageManifest.ts#L314-L334>.
#[test]
fn publish_directory_match_satisfies() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    publishDirectory: ./dist"
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
        "publishConfig": { "directory": "./dist" },
        "dependencies": { "foo": "1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false).is_ok());
}

/// Same dep listed in both `dependencies` and `devDependencies` on
/// the manifest, only in `dependencies` on the lockfile — should
/// pass because upstream's per-field check filters out a dep from
/// `devDependencies` when it also exists in `dependencies` /
/// `optionalDependencies` (precedence: optional > prod > dev).
/// Mirrors upstream's `pkgDepNames` filter at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L69-L84>.
/// Ports the upstream test at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/test/satisfiesPackageManifest.ts#L211-L230>.
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
    // Manifest lists foo under both prod and dev; lockfile records
    // it only under prod (the higher-precedence field).
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "1.0.0" },
        "devDependencies": { "foo": "1.0.0" }
    }"#,
    );
    assert!(
        satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false).is_ok(),
        "manifest listing foo in prod+dev must satisfy a lockfile that records it under prod only",
    );
}

/// Same dep in both `dependencies` and `optionalDependencies`:
/// optional wins precedence, lockfile records it only under
/// `optionalDependencies`. Verifies the precedence rule in the
/// other direction.
#[test]
fn same_dep_in_prod_and_optional_counts_under_optional() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
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
        "optionalDependencies": { "foo": "1.0.0" }
    }"#,
    );
    assert!(
        satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false).is_ok(),
        "manifest listing foo in prod+optional must satisfy a lockfile that records it under optional only",
    );
}

/// Manifest has prod-only deps; lockfile has prod deps plus an
/// empty `devDependencies` map. Should satisfy — absent and empty
/// must be treated alike on the importer side too. Ports the
/// upstream test at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/test/satisfiesPackageManifest.ts#L20-L31>.
#[test]
fn importer_empty_dev_dependencies_equivalent_to_absent() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      foo:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
        "    devDependencies: {}"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "^1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false).is_ok());
}

// ---------------------------------------------------------------------------
// `ignoredOptionalDependencies` — umbrella <https://github.com/pnpm/pacquet/issues/434> slice 7
// ---------------------------------------------------------------------------

/// Sorted equality: both sides empty (`None` and `[]` equivalent).
/// Mirrors upstream's
/// [`getOutdatedLockfileSetting.ts:58-60`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L58-L60).
#[test]
fn check_settings_passes_when_both_sides_empty() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH
        )
        .is_ok(),
    );
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            Some(&[]),
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH
        )
        .is_ok(),
    );
}

/// Order-insensitive compare — upstream sorts both arrays before
/// the equality check, so a config that lists patterns in a
/// different order must still pass.
#[test]
fn check_settings_passes_when_sets_match_regardless_of_order() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "ignoredOptionalDependencies:"
        "  - foo"
        "  - bar"
    })
    .expect("parse lockfile with ignoredOptionalDependencies");
    let config_set = ["bar".to_string(), "foo".to_string()];
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            Some(&config_set),
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// Set mismatch surfaces as `IgnoredOptionalDependenciesChanged`.
#[test]
fn check_settings_returns_drift_when_sets_differ() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "ignoredOptionalDependencies:"
        "  - foo"
    })
    .expect("parse lockfile with ignoredOptionalDependencies");
    let config_set = ["bar".to_string()];
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        Some(&config_set),
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("set drift must surface as IgnoredOptionalDependenciesChanged");
    assert_eq!(
        err,
        StalenessReason::IgnoredOptionalDependenciesChanged {
            lockfile: vec!["foo".to_string()],
            config: vec!["bar".to_string()],
        },
    );
}

/// Drift in the "lockfile has, config doesn't" direction.
#[test]
fn check_settings_returns_drift_when_lockfile_has_set_but_config_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "ignoredOptionalDependencies:"
        "  - foo"
    })
    .expect("parse lockfile with ignoredOptionalDependencies");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("removing a set in config while lockfile has it must surface drift");
    let StalenessReason::IgnoredOptionalDependenciesChanged { lockfile: l, config: c } = err else {
        panic!("expected IgnoredOptionalDependenciesChanged");
    };
    assert_eq!(l, vec!["foo".to_string()]);
    assert!(c.is_empty());
}

// ---------------------------------------------------------------------------
// `overrides` drift — pacquet's lockfile-side mirror of upstream's
// `getOutdatedLockfileSetting` overrides check
// ---------------------------------------------------------------------------

/// Both sides empty / absent → no drift. Mirrors upstream's
/// `equals(lockfile.overrides ?? {}, overrides ?? {})` semantics: a
/// missing key on either side is treated as the empty map.
#[test]
fn check_settings_passes_when_overrides_both_empty() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH
        )
        .is_ok(),
    );

    let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    assert!(
        check_lockfile_settings(
            &lockfile,
            Some(&empty),
            None,
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// Identical maps pass regardless of key insertion order — the
/// comparison normalizes through `BTreeMap`. Mirrors upstream's
/// order-insensitive `equals` from Ramda.
#[test]
fn check_settings_passes_when_overrides_match_regardless_of_order() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
        "  bar: 2.0.0"
    })
    .expect("parse lockfile with overrides");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("bar".to_string(), "2.0.0".to_string());
    config.insert("foo".to_string(), "1.0.0".to_string());
    assert!(
        check_lockfile_settings(
            &lockfile,
            Some(&config),
            None,
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// Value mismatch on a shared key surfaces as `OverridesChanged`.
#[test]
fn check_settings_returns_drift_on_overrides_value_change() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
    })
    .expect("parse lockfile with overrides");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("foo".to_string(), "2.0.0".to_string());
    let err = check_lockfile_settings(
        &lockfile,
        Some(&config),
        None,
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("changed override value must surface drift");
    let StalenessReason::OverridesChanged { lockfile: l, config: c } = err else {
        panic!("expected OverridesChanged");
    };
    assert_eq!(l.get("foo").map(String::as_str), Some("1.0.0"));
    assert_eq!(c.get("foo").map(String::as_str), Some("2.0.0"));
}

/// Lockfile has an override that config no longer does → drift.
#[test]
fn check_settings_returns_drift_when_lockfile_has_overrides_but_config_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
    })
    .expect("parse lockfile with overrides");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("dropped override must surface drift");
    let StalenessReason::OverridesChanged { lockfile: l, config: c } = err else {
        panic!("expected OverridesChanged");
    };
    assert_eq!(l.get("foo").map(String::as_str), Some("1.0.0"));
    assert!(c.is_empty());
}

/// Config has an override that lockfile doesn't → drift.
#[test]
fn check_settings_returns_drift_when_config_has_overrides_but_lockfile_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("foo".to_string(), "1.0.0".to_string());
    let err = check_lockfile_settings(
        &lockfile,
        Some(&config),
        None,
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("added override must surface drift");
    let StalenessReason::OverridesChanged { lockfile: l, config: c } = err else {
        panic!("expected OverridesChanged");
    };
    assert!(l.is_empty());
    assert_eq!(c.get("foo").map(String::as_str), Some("1.0.0"));
}

// ---------------------------------------------------------------------------
// `patchedDependencies` drift — pacquet's lockfile-side mirror of
// upstream's `getOutdatedLockfileSetting` patchedDependencies check
// ---------------------------------------------------------------------------

/// Matching `patchedDependencies` maps pass; the comparison is
/// order-insensitive via `BTreeMap`. Mirrors upstream's
/// `!equals(lockfile.patchedDependencies ?? {}, patchedDependencies ?? {})`.
#[test]
fn check_settings_passes_when_patched_dependencies_match() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "patchedDependencies:"
        "  graceful-fs@4.2.11: abc123"
    })
    .expect("parse lockfile with patchedDependencies");
    let config = std::collections::BTreeMap::from([(
        "graceful-fs@4.2.11".to_string(),
        "abc123".to_string(),
    )]);
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            None,
            Some(&config),
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// A changed patch-file hash (e.g. the user edited the patch) surfaces
/// as `PatchedDependenciesChanged` so the frozen install is rejected
/// rather than silently materializing against a stale `(patch_hash=...)`.
#[test]
fn check_settings_returns_drift_when_patch_hash_changes() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "patchedDependencies:"
        "  graceful-fs@4.2.11: oldhash"
    })
    .expect("parse lockfile with patchedDependencies");
    let config = std::collections::BTreeMap::from([(
        "graceful-fs@4.2.11".to_string(),
        "newhash".to_string(),
    )]);
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        None,
        Some(&config),
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("changed patch hash must surface drift");
    let StalenessReason::PatchedDependenciesChanged { lockfile: l, config: c } = err else {
        panic!("expected PatchedDependenciesChanged, got {err:?}");
    };
    assert_eq!(l.get("graceful-fs@4.2.11").map(String::as_str), Some("oldhash"));
    assert_eq!(c.get("graceful-fs@4.2.11").map(String::as_str), Some("newhash"));
}

/// Config drops a patch the lockfile recorded → drift; absent on the
/// config side normalizes to the empty map.
#[test]
fn check_settings_returns_drift_when_patch_removed_from_config() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "patchedDependencies:"
        "  graceful-fs@4.2.11: abc123"
    })
    .expect("parse lockfile with patchedDependencies");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("dropped patch must surface drift");
    let StalenessReason::PatchedDependenciesChanged { lockfile: l, config: c } = err else {
        panic!("expected PatchedDependenciesChanged, got {err:?}");
    };
    assert_eq!(l.get("graceful-fs@4.2.11").map(String::as_str), Some("abc123"));
    assert!(c.is_empty());
}

/// No `packageExtensionsChecksum` on either side is the steady
/// state for the default install; the gate must accept it.
#[test]
fn check_settings_returns_ok_when_no_package_extensions_checksum_on_either_side() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// Lockfile has a recorded checksum and the current config produces
/// the same checksum → no drift.
#[test]
fn check_settings_returns_ok_when_package_extensions_checksum_matches() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "packageExtensionsChecksum: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    })
    .expect("parse lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// Lockfile recorded `packageExtensionsChecksum: X`, config produces
/// `Y` → drift. Mirrors upstream's `lockfile.packageExtensionsChecksum
/// !== packageExtensionsChecksum` branch.
#[test]
fn check_settings_returns_drift_on_package_extensions_checksum_value_change() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "packageExtensionsChecksum: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    })
    .expect("parse lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        Some("sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="),
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("changed checksum must surface drift");
    let StalenessReason::PackageExtensionsChecksumChanged { lockfile: l, config: c } = err else {
        panic!("expected PackageExtensionsChecksumChanged, got {err:?}");
    };
    assert_eq!(l.as_deref(), Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="));
    assert_eq!(c.as_deref(), Some("sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="));
}

/// Lockfile carries a checksum but the config no longer configures
/// extensions → drift. Symmetric with the
/// "config has but lockfile doesn't" case.
#[test]
fn check_settings_returns_drift_when_lockfile_has_checksum_but_config_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "packageExtensionsChecksum: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    })
    .expect("parse lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("dropped extensions must surface drift");
    let StalenessReason::PackageExtensionsChecksumChanged { lockfile: l, config: c } = err else {
        panic!("expected PackageExtensionsChecksumChanged, got {err:?}");
    };
    assert_eq!(l.as_deref(), Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="));
    assert!(c.is_none());
}

/// Config now produces a checksum that the lockfile doesn't carry →
/// drift.
#[test]
fn check_settings_returns_drift_when_config_has_checksum_but_lockfile_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("added extensions must surface drift");
    let StalenessReason::PackageExtensionsChecksumChanged { lockfile: l, config: c } = err else {
        panic!("expected PackageExtensionsChecksumChanged, got {err:?}");
    };
    assert!(l.is_none());
    assert_eq!(c.as_deref(), Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="));
}

/// `overrides` is checked before `ignoredOptionalDependencies` — when
/// both have drifted, the overrides drift is the one reported, matching
/// upstream's check ordering at
/// <https://github.com/pnpm/pnpm/blob/606f53e78f/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L50-L56>.
#[test]
fn check_settings_reports_overrides_before_ignored_optional() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
        "ignoredOptionalDependencies:"
        "  - bar"
    })
    .expect("parse lockfile");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("foo".to_string(), "2.0.0".to_string());
    let ignored: [String; 0] = [];
    let err = check_lockfile_settings(
        &lockfile,
        Some(&config),
        None,
        Some(&ignored),
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("both drifted; expect OverridesChanged surfaced");
    assert!(
        matches!(err, StalenessReason::OverridesChanged { .. }),
        "expected OverridesChanged first, got {err:?}",
    );
}

// ---------------------------------------------------------------------------
// `injectWorkspacePackages` drift — pacquet's lockfile-side mirror of
// upstream's `getOutdatedLockfileSetting.ts:80-82` Boolean-normalized
// comparison.
// ---------------------------------------------------------------------------

/// Both sides false → no drift. Pacquet's wire format omits the
/// `settings.injectWorkspacePackages` key when `false`, so a lockfile
/// missing the field entirely deserializes to `false` and compares
/// equal to a config that also has it off.
#[test]
fn check_settings_passes_when_inject_workspace_packages_both_false() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// Both sides true → no drift. The lockfile records the setting
/// explicitly (`settings.injectWorkspacePackages: true`) and the
/// current config asserts the same.
#[test]
fn check_settings_passes_when_inject_workspace_packages_both_true() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  injectWorkspacePackages: true"
    })
    .expect("parse lockfile with inject on");
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            None,
            None,
            true,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        )
        .is_ok(),
    );
}

/// Config flipped from `false` to `true` since the lockfile was
/// written → drift surfaces as `InjectWorkspacePackagesChanged`.
#[test]
fn check_settings_returns_drift_when_config_enables_inject_workspace_packages() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        None,
        None,
        true,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("enabling inject must surface drift");
    assert_eq!(
        err,
        StalenessReason::InjectWorkspacePackagesChanged { lockfile: false, config: true },
    );
}

/// Lockfile recorded `injectWorkspacePackages: true` but the user has
/// since disabled it → drift surfaces.
#[test]
fn check_settings_returns_drift_when_config_disables_inject_workspace_packages() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  injectWorkspacePackages: true"
    })
    .expect("parse lockfile with inject on");
    let err = check_lockfile_settings(
        &lockfile,
        None,
        None,
        None,
        None,
        false,
        crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    )
    .expect_err("disabling inject must surface drift");
    assert_eq!(
        err,
        StalenessReason::InjectWorkspacePackagesChanged { lockfile: true, config: false },
    );
}

// ---------------------------------------------------------------------------
// `peersSuffixMaxLength` drift — pacquet's mirror of upstream's
// `getOutdatedLockfileSetting` peersSuffixMaxLength check
// ---------------------------------------------------------------------------

/// Lockfile carries no `settings.peersSuffixMaxLength` field and the
/// config uses the default (1000) — no drift. Mirrors upstream's
/// "unset == default" decay.
#[test]
fn check_settings_passes_when_peers_suffix_max_length_unset_and_config_is_default() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            None,
            None,
            None,
            None,
            false,
            crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH
        )
        .is_ok(),
    );
}

/// Lockfile carries no `settings.peersSuffixMaxLength` (writer used
/// the default — pnpm strips the field at that point), but the current
/// config asks for a non-default value. That's drift: the recorded
/// dep paths assume 1000; re-resolving under a different cap would
/// produce a different graph. Mirrors upstream's
/// [`getOutdatedLockfileSetting.ts`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts)
/// `lockfile.settings?.peersSuffixMaxLength == null && peersSuffixMaxLength !== 1000`
/// branch.
#[test]
fn check_settings_returns_drift_when_lockfile_implicit_default_differs_from_config() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let err = check_lockfile_settings(&lockfile, None, None, None, None, false, 10)
        .expect_err("config != default must surface drift when lockfile is unset");
    assert_eq!(
        err,
        StalenessReason::PeersSuffixMaxLengthChanged {
            lockfile: crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
            config: 10,
        },
    );
}

/// Lockfile explicitly recorded a non-default value and the current
/// config still picks the same value — no drift.
#[test]
fn check_settings_passes_when_explicit_peers_suffix_max_length_matches() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  peersSuffixMaxLength: 10"
    })
    .expect("parse lockfile with settings");
    assert!(check_lockfile_settings(&lockfile, None, None, None, None, false, 10).is_ok());
}

/// Lockfile explicitly recorded one value, current config picks a
/// different one → drift. Mirrors upstream's
/// `lockfile.settings?.peersSuffixMaxLength != null && lockfile.settings.peersSuffixMaxLength !== peersSuffixMaxLength`
/// branch.
#[test]
fn check_settings_returns_drift_when_explicit_peers_suffix_max_length_differs() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  peersSuffixMaxLength: 10"
    })
    .expect("parse lockfile with settings");
    let err = check_lockfile_settings(&lockfile, None, None, None, None, false, 100)
        .expect_err("changed peersSuffixMaxLength must surface drift");
    assert_eq!(err, StalenessReason::PeersSuffixMaxLengthChanged { lockfile: 10, config: 100 });
}

/// Once `check_lockfile_settings` passes, `satisfies_package_manifest`
/// must apply the same filter on the manifest side so an entry the
/// user listed in `ignoredOptionalDependencies` doesn't falsely
/// surface as drift (the lockfile importer correctly doesn't have
/// it, the manifest still does). Mirrors upstream's
/// [`createOptionalDependenciesRemover`](https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts)
/// applied at manifest-read time.
#[test]
fn ignored_optional_filtered_out_of_manifest_diff() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      bar:"
        "        specifier: ^2.0.0"
        "        version: 2.0.0"
    })
    .expect("parse lockfile");
    let importer = lockfile.root_project().expect("root importer");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "foo": "^1.0.0" }
    }"#,
    );
    let is_ignored: &dyn Fn(&str) -> bool = &|name: &str| name == "foo";
    assert!(satisfies_package_manifest(importer, &manifest, ".", is_ignored).is_ok());
}

/// Polarity: without the filter the same fixture must fail.
/// Confirms the filter is what makes the prior test pass — not
/// some other accidental match.
#[test]
fn ignored_optional_without_filter_surfaces_as_drift() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      bar:"
        "        specifier: ^2.0.0"
        "        version: 2.0.0"
    })
    .expect("parse lockfile");
    let importer = lockfile.root_project().expect("root importer");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "foo": "^1.0.0" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, ".", &|_: &str| false)
        .expect_err("without the filter the manifest's extra `foo` must surface as drift");
    assert!(
        matches!(err, StalenessReason::SpecifiersDiffer(_)),
        "expected SpecifiersDiffer, got {err:?}",
    );
}

/// Lockfile serde round-trip: the field is at the top level (not
/// inside `settings`) and round-trips through yaml verbatim, in
/// declaration order. Mirrors upstream's
/// [`LockfileBase.ignoredOptionalDependencies`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/types/src/index.ts#L19)
/// wire shape.
#[test]
fn ignored_optional_dependencies_round_trips_through_yaml() {
    let yaml = text_block! {
        "lockfileVersion: '9.0'"
        "ignoredOptionalDependencies:"
        "  - foo"
        "  - '@scope/bar'"
    };
    let parsed: Lockfile = serde_saphyr::from_str(yaml).expect("parse lockfile");
    assert_eq!(
        parsed.ignored_optional_dependencies.as_deref(),
        Some(&["foo".to_string(), "@scope/bar".to_string()][..]),
    );
}

/// `ignoredOptionalDependencies` must **not** apply to
/// `devDependencies` — upstream's
/// [`createOptionalDependenciesRemover`](https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts)
/// iterates `optionalDependencies` keys and deletes from
/// `optionalDependencies` + `dependencies` only, never touching
/// `devDependencies`. Regression for `CodeRabbit` review on PR [#507].
///
/// Fixture: same name `foo` in both `optionalDependencies` and
/// `devDependencies` on the manifest; lockfile has `foo` only in
/// dev (resolver dropped the optional via the hook, kept the dev).
/// Filter says `foo` is ignored. Without the group gate, the
/// manifest's dev `foo` would be filtered too → diff would flag
/// lockfile's dev `foo` as removed → false drift.
///
/// [#507]: https://github.com/pnpm/pacquet/pull/507
#[test]
fn ignored_optional_does_not_apply_to_dev_dependencies() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    devDependencies:"
        "      foo:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse lockfile");
    let importer = lockfile.root_project().expect("root importer");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "optionalDependencies": { "foo": "^1.0.0" },
        "devDependencies": { "foo": "^1.0.0" }
    }"#,
    );
    let is_ignored: &dyn Fn(&str) -> bool = &|name: &str| name == "foo";
    assert!(satisfies_package_manifest(importer, &manifest, ".", is_ignored).is_ok());
}

/// Mirror sanity: if the filter incorrectly applied to dev (the
/// pre-CodeRabbit-fix behavior), this same fixture without the
/// group gate would flag drift. Used to pin that the gate exists
/// — removing the `matches!(... Prod | Optional)` check inside
/// `satisfies_package_manifest` makes this test fail.
#[test]
fn ignored_optional_dev_only_lockfile_entry_kept() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    devDependencies:"
        "      foo:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse lockfile");
    let importer = lockfile.root_project().expect("root importer");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "devDependencies": { "foo": "^1.0.0" }
    }"#,
    );
    // The manifest doesn't have `foo` in optionalDependencies, so
    // upstream's hook wouldn't iterate it. Filter says `foo` matches
    // a pattern — exercising the case "pattern says foo is ignored
    // but manifest's only entry for foo is in devDependencies".
    // Should pass (dev entry untouched).
    let is_ignored: &dyn Fn(&str) -> bool = &|name: &str| name == "foo";
    assert!(satisfies_package_manifest(importer, &manifest, ".", is_ignored).is_ok());
}
