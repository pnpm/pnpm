use super::{WITH_CATALOG_DEP, manifest_from, parsed_lockfile, try_fast_update_importers};
use pnpm_lockfile::PkgName;
use serde_json::json;

#[test]
fn prunes_a_catalog_entry_its_last_referent_dropped() {
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_CATALOG_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("dropping the catalog referent needs no resolution");

    assert!(updated.catalogs.is_none(), "the orphaned catalog entry goes with its referent");
}
#[test]
fn keeps_a_catalog_entry_another_importer_references() {
    let mut lockfile = parsed_lockfile(WITH_CATALOG_DEP);
    let importer = lockfile.importers["."].clone();
    lockfile.importers.insert("pkg-a".to_string(), importer);
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "catalog:", "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &lockfile,
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("the other importer still references the catalog entry");

    assert!(
        updated.catalogs.as_ref().is_some_and(|catalogs| catalogs["default"].contains_key("foo")),
        "the still-referenced catalog entry stays",
    );
}
#[test]
fn keeps_a_catalog_entry_referenced_with_the_catalog_default_spelling() {
    let mut lockfile = parsed_lockfile(WITH_CATALOG_DEP);
    let mut importer = lockfile.importers["."].clone();
    let alias: PkgName = "foo".parse().expect("alias");
    importer.dependencies.as_mut().expect("dependencies").get_mut(&alias).expect("foo").specifier =
        "catalog:default".to_string();
    lockfile.importers.insert("pkg-a".to_string(), importer);
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));
    let other =
        manifest_from(json!({ "dependencies": { "foo": "catalog:default", "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &lockfile,
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("the other importer still references the catalog entry");

    assert!(
        updated.catalogs.as_ref().is_some_and(|catalogs| catalogs["default"].contains_key("foo")),
        "the catalog:default spelling counts as a reference to the default catalog",
    );
}
