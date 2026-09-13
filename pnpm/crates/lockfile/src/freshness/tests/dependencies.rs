use super::{
    BTreeMap, Lockfile, StalenessReason, assert_eq, manifest_from_json, satisfies_package_manifest,
    text_block,
};

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
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::DependenciesMetaMismatch { .. }),
        "expected DependenciesMetaMismatch, got {err:?}",
    );
}

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
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "1.0.0" },
        "dependenciesMeta": {}
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

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
        satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok(),
        "manifest listing foo in prod+optional must satisfy a lockfile that records it under optional only",
    );
}

#[test]
fn dev_only_dependency_match_satisfies() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    devDependencies:"
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
        "devDependencies": { "foo": "1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

#[test]
fn optional_only_dependency_match_satisfies() {
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
        "optionalDependencies": { "foo": "1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

// ---------------------------------------------------------------------------
// `auto-install-peers` — peers materialized into the importer's
// `dependencies` must not read as lockfile drift. Ports
// `packages/lockfile/verification/test/satisfiesPackageManifest.ts`.
// ---------------------------------------------------------------------------

#[test]
fn peer_only_dependency_is_satisfied_when_auto_install_peers() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      foo:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "      bar:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "1.0.0" },
        "peerDependencies": { "bar": "^1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

#[test]
fn peers_also_declared_as_regular_deps_still_satisfy() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      qar:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "    optionalDependencies:"
        "      bar:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "    devDependencies:"
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
        "dependencies": { "qar": "1.0.0" },
        "optionalDependencies": { "bar": "1.0.0" },
        "devDependencies": { "foo": "1.0.0" },
        "peerDependencies": { "foo": "^1.0.0", "bar": "^1.0.0", "qar": "^1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

#[test]
fn peer_only_dependency_is_stale_without_auto_install_peers() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      foo:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "      bar:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "1.0.0" },
        "peerDependencies": { "bar": "^1.0.0" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, false, &|_: &str| false)
        .expect_err("without auto-install-peers, the materialized peer is unexplained drift");
    let StalenessReason::SpecifiersDiffer(diff) = err else {
        panic!("expected SpecifiersDiffer, got {err:?}");
    };
    assert_eq!(diff.removed, BTreeMap::from([("bar".to_string(), "^1.0.0".to_string())]));
    assert!(diff.added.is_empty());
    assert!(diff.modified.is_empty());
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
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("without the filter the manifest's extra `foo` must surface as drift");
    assert!(
        matches!(err, StalenessReason::SpecifiersDiffer(_)),
        "expected SpecifiersDiffer, got {err:?}",
    );
}

/// Lockfile serde round-trip: the field is at the top level (not
/// inside `settings`) and round-trips through yaml verbatim, in
/// declaration order.
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
/// `devDependencies`: the removal iterates `optionalDependencies` keys
/// and deletes from `optionalDependencies` + `dependencies` only, never
/// touching `devDependencies`. Without the group gate, a dev entry
/// sharing a name with an ignored optional would be filtered too,
/// flagging the lockfile's dev entry as removed → false drift.
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
    assert!(satisfies_package_manifest(importer, &manifest, true, is_ignored).is_ok());
}
