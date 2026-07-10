use super::{
    ExtractManifestError, extract_manifest_from_packed, is_tarball_path, normalize_entry_path,
};
use flate2::{Compression, write::GzEncoder};
use pretty_assertions::assert_eq;
use std::io::Write;
use tempfile::TempDir;

fn write_tarball(dir: &TempDir, entries: &[(&str, &str)]) -> String {
    let path = dir.path().join("pkg.tgz");
    let file = std::fs::File::create(&path).unwrap();
    let mut builder = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    for (name, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, name, contents.as_bytes()).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap().flush().unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn recognizes_tarball_suffixes() {
    assert!(is_tarball_path("foo.tgz"));
    assert!(is_tarball_path("foo-1.0.0.tar.gz"));
    assert!(!is_tarball_path("foo.zip"));
}

#[test]
fn extracts_manifest() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/index.js", "module.exports = 1"),
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
        ],
    );
    let manifest = extract_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["name"], "foo");
    assert_eq!(manifest["version"], "1.0.0");
}

#[test]
fn extracts_manifest_from_non_canonical_entry_path() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(&dir, &[("package/./package.json", r#"{"name":"foo"}"#)]);
    let manifest = extract_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["name"], "foo");
}

#[test]
fn normalize_entry_path_matches_node_path_normalize() {
    use std::path::Path;
    // Collapses `.` and resolvable `..`, matching the relative target.
    assert_eq!(normalize_entry_path(Path::new("package/./package.json")), "package/package.json");
    assert_eq!(
        normalize_entry_path(Path::new("package/sub/../package.json")),
        "package/package.json",
    );
    // Keeps a leading `/` and an unresolvable leading `..`, so neither matches
    // the relative `package/package.json` the lookup compares against.
    assert_eq!(normalize_entry_path(Path::new("/package/package.json")), "/package/package.json");
    assert_eq!(
        normalize_entry_path(Path::new("../package/package.json")),
        "../package/package.json",
    );
}

#[test]
fn errors_when_manifest_missing() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(&dir, &[("package/index.js", "x")]);
    let err = extract_manifest_from_packed(&path).unwrap_err();
    assert!(matches!(err, ExtractManifestError::MissingManifest(_)));
}
