use pacquet_store_dir::{StoreDir, StoreIndex};
use tempfile::TempDir;

use super::{ResolvedPackage, compute_diff};

fn empty_store() -> (TempDir, StoreDir, StoreIndex) {
    let temp = TempDir::new().expect("temp dir");
    let store_dir = StoreDir::new(temp.path().join("store"));
    let store = StoreIndex::open_in(&store_dir).expect("open store index");
    (temp, store_dir, store)
}

#[test]
fn all_present_packages_skip_index_decoding() {
    let (_temp, _store_dir, store) = empty_store();
    let packages = vec![
        ResolvedPackage {
            integrity: "sha512-present-a".to_string(),
            pkg_id: "a@1.0.0".to_string(),
        },
        ResolvedPackage {
            integrity: "sha512-present-b".to_string(),
            pkg_id: "b@1.0.0".to_string(),
        },
    ];
    let result = compute_diff(
        &store,
        &packages,
        &["sha512-present-a".to_string(), "sha512-present-b".to_string()],
    )
    .expect("diff");

    assert_eq!(result.stats.total_packages, 2);
    assert_eq!(result.stats.already_in_store, 2);
    assert!(result.missing_files.is_empty());
    assert!(result.package_index.is_empty());
}

#[test]
fn missing_package_keeps_existing_miss_behavior() {
    let (_temp, _store_dir, store) = empty_store();
    let packages = vec![ResolvedPackage {
        integrity: "sha512-missing".to_string(),
        pkg_id: "missing@1.0.0".to_string(),
    }];
    let result = compute_diff(&store, &packages, &[]).expect("diff");

    assert_eq!(result.stats.total_packages, 1);
    assert_eq!(result.stats.already_in_store, 0);
    assert!(result.missing_files.is_empty());
    assert!(result.package_index.is_empty());
}
