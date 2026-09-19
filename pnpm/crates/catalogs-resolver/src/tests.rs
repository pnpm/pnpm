use std::path::Path;

use super::{
    CatalogAnchor, CatalogResolution, CatalogResolutionError, CatalogResolutionFound,
    CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_catalogs_types::{Catalog, Catalogs};

fn catalogs_from(entries: &[(&str, &[(&str, &str)])]) -> Catalogs {
    entries
        .iter()
        .map(|(name, items)| {
            let catalog: Catalog = items
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect();
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
        resolve_from_catalog(&catalogs, &wanted("foo", "catalog:"), CatalogAnchor::AsWritten),
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
        resolve_from_catalog(
            &catalogs,
            &wanted("foo", "catalog:default"),
            CatalogAnchor::AsWritten
        ),
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
        resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"), CatalogAnchor::AsWritten),
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
        resolve_from_catalog(&catalogs, &wanted("bar", "^2.0.0"), CatalogAnchor::AsWritten),
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
        let result =
            resolve_from_catalog(&catalogs, &wanted(alias, bare), CatalogAnchor::AsWritten);
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
    let result =
        resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"), CatalogAnchor::AsWritten);
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
fn resolves_workspace_protocol_from_catalog() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "workspace:*")])]);
    assert_eq!(
        resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"), CatalogAnchor::AsWritten),
        CatalogResolutionResult::Found(CatalogResolutionFound {
            resolution: CatalogResolution {
                catalog_name: "foo".to_string(),
                specifier: "workspace:*".to_string(),
            },
        }),
    );
}

#[cfg(windows)]
const WORKSPACE_DIR: &str = r"C:\workspace";
#[cfg(not(windows))]
const WORKSPACE_DIR: &str = "/workspace";

fn reanchored(entry: &str, consumer: Option<&str>) -> String {
    let catalogs = catalogs_from(&[("foo", &[("bar", entry)])]);
    let workspace_dir = Path::new(WORKSPACE_DIR);
    let consumer_dir = consumer.map(|dir| workspace_dir.join(dir));
    let anchor = CatalogAnchor::Reanchor { workspace_dir, consumer_dir: consumer_dir.as_deref() };
    match resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"), anchor) {
        CatalogResolutionResult::Found(found) => found.resolution.specifier,
        other => panic!("expected the entry to resolve, got {other:?}"),
    }
}

#[test]
fn reanchors_a_file_entry_on_the_consuming_project() {
    assert_eq!(
        reanchored("file:./tarballs/bar-1.0.0.tgz", Some("packages/foo")),
        "file:../../tarballs/bar-1.0.0.tgz",
    );
}

#[test]
fn reanchors_a_link_entry_on_the_consuming_project() {
    assert_eq!(reanchored("link:libs/bar", Some("packages/foo")), "link:../../libs/bar");
}

#[test]
fn renders_a_local_entry_absolute_for_a_consumer_outside_the_workspace() {
    let expected = format!("file:{}/tarballs/bar-1.0.0.tgz", WORKSPACE_DIR.replace('\\', "/"));
    assert_eq!(reanchored("file:./tarballs/bar-1.0.0.tgz", None), expected);
}

#[test]
fn leaves_a_local_entry_alone_when_the_anchor_is_as_written() {
    let catalogs = catalogs_from(&[("foo", &[("bar", "file:./tarballs/bar-1.0.0.tgz")])]);
    assert_eq!(
        resolve_from_catalog(&catalogs, &wanted("bar", "catalog:foo"), CatalogAnchor::AsWritten),
        CatalogResolutionResult::Found(CatalogResolutionFound {
            resolution: CatalogResolution {
                catalog_name: "foo".to_string(),
                specifier: "file:./tarballs/bar-1.0.0.tgz".to_string(),
            },
        }),
    );
}

#[test]
fn leaves_a_registry_entry_alone_while_reanchoring() {
    assert_eq!(reanchored("^1.2.3", Some("packages/foo")), "^1.2.3");
    assert_eq!(reanchored("workspace:*", Some("packages/foo")), "workspace:*");
    assert_eq!(reanchored("npm:other@^1", Some("packages/foo")), "npm:other@^1");
}

#[test]
fn reanchors_a_bare_local_path_entry_on_the_consuming_project() {
    assert_eq!(
        reanchored("./tarballs/bar-1.0.0.tgz", Some("packages/foo")),
        "../../tarballs/bar-1.0.0.tgz",
    );
}

/// A shape the resolver chain reaches through another resolver keeps
/// its own meaning: a hosted-git shorthand is not a directory in the
/// workspace, with or without a tarball suffix.
#[test]
fn leaves_a_git_shorthand_entry_alone_while_reanchoring() {
    assert_eq!(reanchored("user/repo", Some("packages/foo")), "user/repo");
    assert_eq!(reanchored("user/repo.tgz", Some("packages/foo")), "user/repo.tgz");
}

/// A single-letter named registry is well-formed, so `c:pkg@1` is a
/// registry specifier as much as it is a Windows drive path.
#[test]
fn leaves_a_named_registry_entry_alone_while_reanchoring() {
    assert_eq!(reanchored("c:pkg@1", Some("packages/foo")), "c:pkg@1");
    assert_eq!(reanchored("gh:@scope/pkg", Some("packages/foo")), "gh:@scope/pkg");
}
