use super::PackageMetadata;
use crate::serialize_yaml;
use text_block_macros::text_block;

fn make_metadata(libc_yaml: &str) -> String {
    let base = text_block! {
        "resolution:"
        "  integrity: sha512-abc123"
        "  tarball: https://registry.npmjs.org/foo/-/foo-1.0.0.tgz"
        "cpu: [arm64]"
        "os: [linux]"
    };
    format!("{base}\n{libc_yaml}")
}

#[test]
fn libc_as_string() {
    let yaml = make_metadata("libc: glibc\n");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(metadata.libc, Some(vec!["glibc".to_string()]));
}

#[test]
fn libc_as_array() {
    let yaml = make_metadata("libc: [glibc]\n");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(metadata.libc, Some(vec!["glibc".to_string()]));
}

#[test]
fn libc_absent() {
    let yaml = make_metadata("");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(metadata.libc, None);
}

#[test]
fn libc_string_roundtrip() {
    let yaml = make_metadata("libc: glibc\n");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    let serialized = serialize_yaml::to_string(&metadata).unwrap();
    let reparsed: PackageMetadata = serde_saphyr::from_str(&serialized).unwrap();
    assert_eq!(metadata.libc, reparsed.libc);
}
