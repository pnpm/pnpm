use super::DistStats;
use crate::Storage;

#[tokio::test]
async fn dist_stats_are_shared_between_hosted_namespaces() {
    let temp = tempfile::TempDir::new().unwrap();
    let first = Storage::new(
        &crate::HostedStoreConfig::Fs,
        temp.path().join("storage"),
        temp.path().join("cache"),
    )
    .unwrap();
    let name =
        crate::CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm).unwrap();
    first
        .record_dist_stats(
            &name,
            "sha512-package",
            DistStats { file_count: 3, unpacked_size: 17, bins: None },
        )
        .await
        .unwrap();

    let stats = first
        .for_hosted("acme")
        .read_dist_stats(&name)
        .await
        .unwrap();
    assert_eq!(
        stats.get("sha512-package"),
        Some(&DistStats { file_count: 3, unpacked_size: 17, bins: None }),
    );
}
