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

#[test]
fn malformed_pnpr_metadata_is_ignored_without_losing_valid_bins() {
    let malformed: PackageDistribution = serde_json::from_value(serde_json::json!({
        "pnpr": "unexpected"
    }))
    .unwrap();
    assert!(malformed.pnpr.is_none());

    let valid: PackageDistribution = serde_json::from_value(serde_json::json!({
        "pnpr": { "bins": [{ "name": "tool", "path": "bin/tool", "shebang": null }] }
    }))
    .unwrap();
    assert_eq!(valid.pnpr.unwrap().bins.unwrap()[0].name, "tool");
}
