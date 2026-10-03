use super::{LocalDepContext, check_recorded_peer_specs_match};
use crate::install::FreshnessCheckError;
use pnpm_catalogs_resolver::CatalogResolutionError;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{PackageMetadata, StalenessReason};
use std::{collections::HashMap, path::Path};

#[test]
fn unchanged_catalog_peer_specs_are_accepted_without_catalogs() {
    let catalogs = Catalogs::new();
    let result = check_peer_spec("catalog:", "catalog:", &catalogs);
    assert!(result.is_ok(), "raw recorded catalog peers must still match: {result:?}");
}

#[test]
fn a_changed_catalog_peer_without_catalogs_is_outdated() {
    let error = check_peer_spec("catalog:", "1.0.0", &Catalogs::new())
        .expect_err("the peer range changed without configured catalogs");
    assert!(
        matches!(
            &error,
            FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated { .. })
        ),
        "expected a stale local dependency, got {error:?}",
    );
}

#[test]
fn missing_catalog_peers_preserve_the_catalog_error() {
    let catalogs = Catalogs::from([(
        "default".to_string(),
        std::collections::BTreeMap::from([("foo".to_string(), "1.0.0".to_string())]),
    )]);
    let error = check_peer_spec("catalog:missing", "1.0.0", &catalogs)
        .expect_err("the named catalog does not exist");
    assert!(
        matches!(
            &error,
            FreshnessCheckError::InvalidCatalog(CatalogResolutionError::EntryNotFoundForSpec {
                alias,
                catalog_name,
            }) if alias == "foo" && catalog_name == "missing"
        ),
        "expected the catalog error, got {error:?}",
    );
}

#[test]
fn a_changed_catalog_peer_range_is_outdated() {
    let catalogs = Catalogs::from([(
        "default".to_string(),
        std::collections::BTreeMap::from([("foo".to_string(), "2.0.0".to_string())]),
    )]);
    let error = check_peer_spec("catalog:", "1.0.0", &catalogs)
        .expect_err("the catalog range differs from the recorded range");
    assert!(
        matches!(
            &error,
            FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated { .. })
        ),
        "expected a stale local dependency, got {error:?}",
    );
}

fn check_peer_spec(
    manifest_spec: &str,
    recorded_spec: &str,
    catalogs: &Catalogs,
) -> Result<(), FreshnessCheckError> {
    let dep = LocalDepContext {
        name: "lib",
        rel_path: "lib",
        dir: Path::new("lib"),
        lockfile_dir: Path::new("."),
        workspace_root: Path::new("."),
        catalogs,
    };
    let metadata: PackageMetadata = serde_json::from_value(serde_json::json!({
        "resolution": { "type": "directory", "directory": "lib" },
        "peerDependencies": { "foo": recorded_spec },
    }))
    .expect("parse local package metadata");
    check_recorded_peer_specs_match(&dep, &HashMap::from([("foo", manifest_spec)]), &metadata)
}
