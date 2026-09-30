use deser_value::Value;

use super::PackageDistribution;

#[test]
fn revision_is_excluded_from_content_equality() {
    let integrity: ssri::Integrity = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
        .parse()
        .unwrap();
    let first = PackageDistribution {
        integrity: Some(integrity.clone()),
        revision: Some(Value::from(1u64)),
        ..PackageDistribution::default()
    };
    let second = PackageDistribution {
        integrity: Some(integrity),
        revision: Some(Value::from(2u64)),
        ..PackageDistribution::default()
    };

    assert_eq!(first, second);
}
