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
