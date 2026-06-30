use super::{
    CatalogResolution, CatalogResolutionError, CatalogResolutionFound, CatalogResolutionResult,
    WantedDependency, resolve_from_catalog,
};
use pacquet_catalogs_types::{Catalog, Catalogs};

fn catalogs_from(entries: &[(&str, &[(&str, &str)])]) -> Catalogs {
    entries
        .iter()
        .map(|(name, items)| {
            let catalog: Catalog =
                items.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
            ((*name).to_string(), catalog)
        })
        .collect()
}

fn wanted(alias: &str, bare_specifier: &str) -> WantedDependency {
    WantedDependency { alias: alias.to_string(), bare_specifier: bare_specifier.to_string() }
}

#[test]
fn default_catalog_resolves_using_implicit_name() {
    let catalogs = catalogs_from(&[("default", &[("foo", "1.0.0")])]);
    assert_eq!(
        resolve_from_catalog(&catalogs, &wanted("foo", "catalog:")),
        CatalogResolutionResult::Found(CatalogResolutionFound {
            resolution: CatalogResolution {
                catalog_name: "default".to_string(),
                specifier: "1.0.0".to_string(),
            },
        }),
    );
}

#[test]
fn default_catalog_resolves_using_explicit_name() {
    let catalogs = catalogs_from(&[("default", &[("foo", "1.0.0")])]);
    assert_eq!(
        resolve_from_catalog(&catalogs, &wanted("foo", "catalog:default")),
        CatalogResolutionResult::Found(CatalogResolutionFound {
            resolution: CatalogResolution {
                catalog_name: "default".to_string(),
                specifier: "1.0.0".to_string(),
            },
        }),
    );
}

#[test]
fn resolves_named_catalog() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "1.0.0")])]);
    assert_eq!(
        resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo")),
        CatalogResolutionResult::Found(CatalogResolutionFound {
            resolution: CatalogResolution {
                catalog_name: "foo".to_string(),
                specifier: "1.0.0".to_string(),
            },
        }),
    );
}

#[test]
fn returns_unused_for_specifier_not_using_catalog_protocol() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "1.0.0")])]);
    assert_eq!(
        resolve_from_catalog(&catalogs, &wanted("bar", "^2.0.0")),
        CatalogResolutionResult::Unused,
    );
}

#[test]
fn returns_error_for_missing_unresolved_catalog() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "1.0.0")])]);
    for (alias, bare, expected_catalog) in [
        ("bar", "catalog:", "default"),
        ("bar", "catalog:baz", "baz"),
        ("foo", "catalog:foo", "foo"),
    ] {
        let result = resolve_from_catalog(&catalogs, &wanted(alias, bare));
        let CatalogResolutionResult::Misconfiguration(misconfig) = &result else {
            panic!("expected misconfiguration for ({alias}, {bare}), got {result:?}");
        };
        assert_eq!(misconfig.catalog_name, expected_catalog);
        assert_eq!(
            misconfig.error,
            CatalogResolutionError::EntryNotFoundForSpec {
                alias: alias.to_string(),
                catalog_name: expected_catalog.to_string(),
            },
        );
        assert_eq!(
            misconfig.error.to_string(),
            format!("No catalog entry '{alias}' was found for catalog '{expected_catalog}'."),
        );
    }
}

#[test]
fn returns_error_for_recursive_catalog() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "catalog:foo")])]);
    let result = resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"));
    let CatalogResolutionResult::Misconfiguration(misconfig) = &result else {
        panic!("expected misconfiguration, got {result:?}");
    };
    assert_eq!(
        misconfig.error,
        CatalogResolutionError::EntryInvalidRecursiveDefinition {
            alias: "bar".to_string(),
            catalog_name: "foo".to_string(),
        },
    );
    assert_eq!(
        misconfig.error.to_string(),
        "Found invalid catalog entry using the catalog protocol recursively. \
         The entry for 'bar' in catalog 'foo' is invalid.",
    );
}

#[test]
fn returns_error_for_workspace_protocol_in_catalog() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "workspace:*")])]);
    let result = resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"));
    let CatalogResolutionResult::Misconfiguration(misconfig) = &result else {
        panic!("expected misconfiguration, got {result:?}");
    };
    assert_eq!(
        misconfig.error,
        CatalogResolutionError::EntryInvalidWorkspaceSpec {
            alias: "bar".to_string(),
            catalog_name: "foo".to_string(),
        },
    );
    assert_eq!(
        misconfig.error.to_string(),
        "The workspace protocol cannot be used as a catalog value. \
         The entry for 'bar' in catalog 'foo' is invalid.",
    );
}

#[test]
fn returns_error_for_file_protocol_in_catalog() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "file:./bar.tgz")])]);
    let result = resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"));
    let CatalogResolutionResult::Misconfiguration(misconfig) = &result else {
        panic!("expected misconfiguration, got {result:?}");
    };
    assert_eq!(
        misconfig.error,
        CatalogResolutionError::EntryInvalidSpec {
            alias: "bar".to_string(),
            catalog_name: "foo".to_string(),
            protocol: "file".to_string(),
        },
    );
    assert_eq!(
        misconfig.error.to_string(),
        "The entry for 'bar' in catalog 'foo' declares a dependency using the 'file' protocol. \
         This is not yet supported, but may be in a future version of pnpm.",
    );
}

#[test]
fn returns_error_for_link_protocol_in_catalog() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "link:./bar")])]);
    let result = resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"));
    let CatalogResolutionResult::Misconfiguration(misconfig) = &result else {
        panic!("expected misconfiguration, got {result:?}");
    };
    assert_eq!(
        misconfig.error,
        CatalogResolutionError::EntryInvalidSpec {
            alias: "bar".to_string(),
            catalog_name: "foo".to_string(),
            protocol: "link".to_string(),
        },
    );
    assert_eq!(
        misconfig.error.to_string(),
        "The entry for 'bar' in catalog 'foo' declares a dependency using the 'link' protocol. \
         This is not yet supported, but may be in a future version of pnpm.",
    );
}
