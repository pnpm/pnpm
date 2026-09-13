use super::{
    Lockfile, StalenessReason, assert_eq, manifest_from_json, satisfies_package_manifest,
    text_block,
};

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
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

#[test]
fn equivalent_git_specifiers_satisfy_manifest() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      is-positive:"
        "        specifier: git+https://github.com/kevva/is-positive.git#97edff6"
        "        version: git+https://github.com/kevva/is-positive.git#97edff6"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": {
            "is-positive": "git://github.com/kevva/is-positive#97edff6"
        }
    }"#,
    );
    satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect("equivalent git specifiers must satisfy the manifest");
}

#[test]
fn different_git_specifiers_do_not_satisfy_manifest() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      is-positive:"
        "        specifier: git+https://github.com/kevva/is-positive.git#97edff6"
        "        version: git+https://github.com/kevva/is-positive.git#97edff6"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    for specifier in [
        "git+https://gitlab.com/kevva/is-positive.git#97edff6",
        "git+https://github.com/kevva/different.git#97edff6",
        "git+https://github.com/kevva/is-positive.git#different",
    ] {
        let (_dir, manifest) = manifest_from_json(&format!(
            r#"{{
            "name": "x",
            "version": "1.0.0",
            "dependencies": {{
                "is-positive": "{specifier}"
            }}
        }}"#,
        ));
        let error = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
            .expect_err("different git specifiers must leave the manifest stale");
        assert!(
            matches!(error, StalenessReason::SpecifiersDiffer(_)),
            "SPECIFIER: {specifier}\nERROR: {error:?}",
        );
    }
}

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
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    let StalenessReason::SpecifiersDiffer(diff) = err else {
        panic!("expected SpecifiersDiffer, got {err:?}");
    };
    assert_eq!(diff.added.get("lodash").map(String::as_str), Some("^4.17.21"));
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
}

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
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    let StalenessReason::SpecifiersDiffer(diff) = err else {
        panic!("expected SpecifiersDiffer, got {err:?}");
    };
    assert_eq!(diff.removed.get("lodash").map(String::as_str), Some("^4.17.21"));
}

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
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    let StalenessReason::SpecifiersDiffer(diff) = err else {
        panic!("expected SpecifiersDiffer, got {err:?}");
    };
    let modified = diff.modified.get("react").expect("react bucketed under modified");
    assert_eq!(modified.0, "^17.0.2");
    assert_eq!(modified.1, "^18.0.0");
}

/// A dependency the manifest lists only under `optionalDependencies`
/// but the lockfile records under `dependencies` is drift: the flat
/// diff agrees on the specifier, so the per-field check must catch the
/// field move. Mirrors the `optionalDependencies` mismatch case in
/// `satisfiesPackageManifest.ts`.
#[test]
fn manifest_optional_only_but_lockfile_records_prod_is_stale() {
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
        "optionalDependencies": { "foo": "1.0.0" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("a dep the manifest lists only as optional but the lockfile records under dependencies must be stale");
    assert!(
        matches!(err, StalenessReason::DepSpecifierMismatch { field: "dependencies", .. }),
        "got: {err:?}",
    );
}

/// Once `check_lockfile_settings` passes, `satisfies_package_manifest`
/// must apply the same filter on the manifest side so an entry the
/// user listed in `ignoredOptionalDependencies` doesn't falsely
/// surface as drift (the lockfile importer correctly doesn't have
/// it, the manifest still does). The filter is applied at
/// manifest-read time.
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
    assert!(satisfies_package_manifest(importer, &manifest, true, is_ignored).is_ok());
}
