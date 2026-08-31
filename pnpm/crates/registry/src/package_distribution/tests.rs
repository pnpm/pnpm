use super::PackageDistribution;

#[test]
fn revision_is_excluded_from_content_equality() {
    let integrity: ssri::Integrity = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
        .parse()
        .unwrap();
    let first = PackageDistribution {
        integrity: Some(integrity.clone()),
        revision: Some(serde_json::json!(1)),
        ..PackageDistribution::default()
    };
    let second = PackageDistribution {
        integrity: Some(integrity),
        revision: Some(serde_json::json!(2)),
        ..PackageDistribution::default()
    };

    assert_eq!(first, second);
}
