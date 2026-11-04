use super::{
    Lockfile, StalenessReason, assert_eq, manifest_from_json, satisfies_package_manifest,
    text_block,
};

#[test]
fn resolved_version_outside_manifest_range_is_stale() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      '@apollo/client':"
        "        specifier: 3.3.7"
        "        version: 3.13.8"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "@apollo/client": "3.3.7" }
    }"#,
    );

    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("a broken direct resolution must be stale");
    assert_eq!(
        err,
        StalenessReason::ResolutionDoesNotSatisfy {
            name: "@apollo/client".to_string(),
            version: "3.13.8".to_string(),
            range: "3.3.7".to_string(),
        },
    );
}

#[test]
fn any_version_range_accepts_a_prerelease_resolution() {
    for spec in ["*", "", "^1.0.0 || "] {
        let lockfile: Lockfile = serde_saphyr::from_str(&format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      only-pre:\n        specifier: '{spec}'\n        version: 2.0.0-alpha.0\n",
        ))
        .expect("parse fixture lockfile");
        let importer = lockfile.root_project().expect("root importer present");
        let manifest_json = serde_json::json!({
            "name": "x",
            "version": "1.0.0",
            "dependencies": { "only-pre": spec },
        });
        let (_dir, manifest) = manifest_from_json(&manifest_json.to_string());

        assert_eq!(
            satisfies_package_manifest(importer, &manifest, true, &|_: &str| false),
            Ok(()),
            "for {spec:?}",
        );
    }
}
