use super::{FastCatalogUpdate, try_fast_update_catalogs};
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::Lockfile;

fn lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
}

#[test]
fn retains_a_version_that_satisfies_the_updated_range() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
catalogs:
  default:
    foo:
      specifier: ^1.0.0
      version: 1.1.0
importers:
  .:
    dependencies:
      foo:
        specifier: 'catalog:'
        version: 1.1.0
",
    );
    let catalogs = Catalogs::from([(
        "default".to_string(),
        [("foo".to_string(), ">=1 <2".to_string())].into(),
    )]);

    let FastCatalogUpdate::Updated(updated) = try_fast_update_catalogs(&lockfile, &catalogs, false)
    else {
        panic!("compatible catalog update should succeed");
    };
    let entry = &updated.catalogs.expect("catalog snapshots")["default"]["foo"];
    assert_eq!(entry.specifier, ">=1 <2");
    assert_eq!(entry.version, "1.1.0");
}

#[test]
fn rejects_an_updated_range_that_excludes_the_locked_version() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
catalogs:
  default:
    foo:
      specifier: ^1.0.0
      version: 1.1.0
importers:
  .:
    dependencies:
      foo:
        specifier: 'catalog:'
        version: 1.1.0
",
    );
    let catalogs =
        Catalogs::from([("default".to_string(), [("foo".to_string(), "^2".to_string())].into())]);

    assert!(matches!(
        try_fast_update_catalogs(&lockfile, &catalogs, false),
        FastCatalogUpdate::Unsupported
    ));
}

#[test]
fn rejects_a_malformed_locked_version() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
catalogs:
  default:
    foo:
      specifier: ^1.0.0
      version: not-a-version
importers:
  .:
    dependencies:
      foo:
        specifier: 'catalog:'
        version: 1.1.0
",
    );
    let catalogs = Catalogs::from([(
        "default".to_string(),
        [("foo".to_string(), ">=1 <2".to_string())].into(),
    )]);

    assert!(matches!(
        try_fast_update_catalogs(&lockfile, &catalogs, false),
        FastCatalogUpdate::Unsupported
    ));
}

#[test]
fn catalog_backed_overrides_do_not_disable_reuse_when_catalogs_are_unchanged() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
catalogs:
  default:
    foo:
      specifier: ^1.0.0
      version: 1.1.0
importers:
  .:
    dependencies:
      foo:
        specifier: 'catalog:'
        version: 1.1.0
",
    );
    let catalogs = Catalogs::from([(
        "default".to_string(),
        [("foo".to_string(), "^1.0.0".to_string())].into(),
    )]);

    assert!(matches!(
        try_fast_update_catalogs(&lockfile, &catalogs, true),
        FastCatalogUpdate::Unchanged
    ));
}

#[test]
fn configured_catalogs_require_existing_lockfile_snapshots() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .: {}
",
    );
    let catalogs =
        Catalogs::from([("default".to_string(), [("foo".to_string(), "^1".to_string())].into())]);

    assert!(matches!(
        try_fast_update_catalogs(&lockfile, &catalogs, false),
        FastCatalogUpdate::Unsupported
    ));
}

#[test]
fn referenced_catalog_entries_require_lockfile_snapshots() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
catalogs:
  default:
    bar:
      specifier: ^1
      version: 1.0.0
importers:
  .:
    dependencies:
      foo:
        specifier: 'catalog:'
        version: 1.0.0
",
    );
    let catalogs =
        Catalogs::from([("default".to_string(), [("foo".to_string(), "^1".to_string())].into())]);

    assert!(matches!(
        try_fast_update_catalogs(&lockfile, &catalogs, false),
        FastCatalogUpdate::Unsupported
    ));
}

#[test]
fn removes_an_unreferenced_stale_snapshot() {
    let lockfile = lockfile(
        r"
lockfileVersion: '9.0'
catalogs:
  default:
    foo:
      specifier: ^1.0.0
      version: 1.1.0
importers:
  .: {}
",
    );

    let FastCatalogUpdate::Updated(updated) =
        try_fast_update_catalogs(&lockfile, &Catalogs::new(), false)
    else {
        panic!("unreferenced snapshot should be removable");
    };
    assert!(updated.catalogs.is_none());
}

#[test]
fn rejects_removing_a_snapshot_referenced_by_the_default_catalog() {
    for specifier in ["catalog:", "catalog:default"] {
        let lockfile = lockfile(&format!(
            r"
lockfileVersion: '9.0'
catalogs:
  default:
    foo:
      specifier: ^1.0.0
      version: 1.1.0
importers:
  .:
    dependencies:
      foo:
        specifier: '{specifier}'
        version: 1.1.0
",
        ));

        let update = try_fast_update_catalogs(&lockfile, &Catalogs::new(), false);
        assert!(
            matches!(update, FastCatalogUpdate::Unsupported),
            "default catalog reference {specifier} should prevent snapshot removal",
        );
    }
}
