use super::PackageTag;

#[test]
fn registry_path_segment_encodes_each_variant() {
    assert_eq!(PackageTag::Latest.registry_path_segment(), "latest");
    assert_eq!("1.2.3".parse::<PackageTag>().unwrap().registry_path_segment(), "1.2.3");
    // `+` build metadata is not path-safe and must be percent-encoded.
    assert_eq!(
        "1.2.3+build.1".parse::<PackageTag>().unwrap().registry_path_segment(),
        "1.2.3%2Bbuild.1",
    );
    assert_eq!(PackageTag::Tag("beta/next".to_owned()).registry_path_segment(), "beta%2Fnext");
}
