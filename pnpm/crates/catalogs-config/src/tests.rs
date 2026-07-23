use super::{InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest};
use pacquet_catalogs_types::{Catalog, Catalogs};
use pacquet_workspace::WorkspaceManifest;

fn catalog_from(entries: &[(&str, &str)]) -> Catalog {
    entries.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}

#[test]
fn combines_implicit_default_and_named_catalogs() {
    let manifest = WorkspaceManifest {
        catalog: Some(catalog_from(&[("foo", "^1.0.0")])),
        catalogs: Some(Catalogs::from([("bar".to_string(), catalog_from(&[("baz", "^2.0.0")]))])),
        ..WorkspaceManifest::default()
    };

    let expected = Catalogs::from([
        ("default".to_string(), catalog_from(&[("foo", "^1.0.0")])),
        ("bar".to_string(), catalog_from(&[("baz", "^2.0.0")])),
    ]);
    assert_eq!(get_catalogs_from_workspace_manifest(Some(&manifest)).unwrap(), expected);
}

#[test]
fn combines_explicit_default_and_named_catalogs() {
    let manifest = WorkspaceManifest {
        catalog: None,
        catalogs: Some(Catalogs::from([
            ("default".to_string(), catalog_from(&[("foo", "^1.0.0")])),
            ("bar".to_string(), catalog_from(&[("baz", "^2.0.0")])),
        ])),
        ..WorkspaceManifest::default()
    };

    let expected = Catalogs::from([
        ("default".to_string(), catalog_from(&[("foo", "^1.0.0")])),
        ("bar".to_string(), catalog_from(&[("baz", "^2.0.0")])),
    ]);
    assert_eq!(get_catalogs_from_workspace_manifest(Some(&manifest)).unwrap(), expected);
}

#[test]
fn throws_if_default_catalog_is_defined_multiple_times() {
    let manifest = WorkspaceManifest {
        catalog: Some(catalog_from(&[("bar", "^2.0.0")])),
        catalogs: Some(Catalogs::from([(
            "default".to_string(),
            catalog_from(&[("foo", "^1.0.0")]),
        )])),
        ..WorkspaceManifest::default()
    };

    let err = get_catalogs_from_workspace_manifest(Some(&manifest)).unwrap_err();
    assert_eq!(err, InvalidCatalogsConfigurationError::DefaultDefinedMultipleTimes);
    assert_eq!(
        err.to_string(),
        "The 'default' catalog was defined multiple times. \
         Use the 'catalog' field or 'catalogs.default', but not both.",
    );
}

#[test]
fn returns_empty_map_for_missing_workspace_manifest() {
    assert_eq!(get_catalogs_from_workspace_manifest(None).unwrap(), Catalogs::new());
}
