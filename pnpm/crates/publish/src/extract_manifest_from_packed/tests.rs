use super::{
    ExtractManifestError, extract_manifest_from_packed, extract_publish_manifest_from_packed,
    is_tarball_path, normalize_entry_path,
};
use flate2::{Compression, write::GzEncoder};
use pretty_assertions::assert_eq;
use std::io::Write;
use tempfile::TempDir;

fn write_tarball(dir: &TempDir, entries: &[(&str, &str)]) -> String {
    let entries = entries
        .iter()
        .map(|(name, contents)| (*name, contents.as_bytes()))
        .collect::<Vec<_>>();
    write_tarball_bytes(dir, &entries)
}

fn write_tarball_bytes(dir: &TempDir, entries: &[(&str, &[u8])]) -> String {
    let path = dir.path().join("pkg.tgz");
    let file = std::fs::File::create(&path).unwrap();
    let mut builder = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    for (name, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(contents.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, name, *contents).unwrap();
    }
    builder
        .into_inner()
        .unwrap()
        .finish()
        .unwrap()
        .flush()
        .unwrap();
    path.to_string_lossy().into_owned()
}

fn write_tarball_with_non_files(dir: &TempDir) -> String {
    let path = dir.path().join("pkg.tgz");
    let file = std::fs::File::create(&path).unwrap();
    let mut builder = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    for (name, contents) in [
        ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
        ("package/README", "# Bare"),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(contents.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, name, contents.as_bytes()).unwrap();
    }
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_cksum();
    builder.append_link(&mut header, "package/README.md", "README").unwrap();
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_size(0);
    header.set_cksum();
    builder
        .append_data(&mut header, "package/readme.markdown", std::io::empty())
        .unwrap();
    builder
        .into_inner()
        .unwrap()
        .finish()
        .unwrap()
        .flush()
        .unwrap();
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

/// `pnpm publish <tarball>` must accept a packed manifest carrying a
/// UTF-8 BOM, the same as one read from a project directory.
#[test]
fn extracts_a_manifest_that_starts_with_a_utf8_bom() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[("package/package.json", "\u{feff}{\"name\":\"foo\",\"version\":\"1.0.0\"}")],
    );
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

#[test]
fn publish_manifest_fills_readme_from_npm_readme_names() {
    for readme_entry in ["package/README.md", "package/README", "package/readme.markdown"] {
        let dir = TempDir::new().unwrap();
        let path = write_tarball(
            &dir,
            &[
                ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
                (readme_entry, "# Hello"),
            ],
        );
        let manifest = extract_publish_manifest_from_packed(&path).unwrap();
        assert_eq!(manifest["readme"], "# Hello", "{readme_entry}");
    }
}

#[test]
fn publish_manifest_prefers_markdown_readme_over_bare_readme() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/README", "# Bare"),
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
            ("package/readme.markdown", "# Markdown"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Markdown");
}

#[test]
fn publish_manifest_keeps_markdown_readme_when_bare_readme_follows() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/readme.markdown", "# Markdown"),
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
            ("package/README", "# Bare"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Markdown");
}

#[test]
fn publish_manifest_prefers_readme_md_when_multiple_readmes_exist() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/readme.markdown", "# Fallback"),
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
            ("package/README.md", "# Preferred"),
            ("package/README", "# Bare"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Preferred");
}

#[test]
fn publish_manifest_prefers_readme_md_over_bare_readme() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/README", "# Bare"),
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
            ("package/README.md", "# Preferred"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Preferred");
}

#[test]
fn publish_manifest_keeps_last_duplicate_readme_md() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
            ("package/README.md", "# First"),
            ("package/README.md", "# Last"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Last");
}

#[test]
fn publish_manifest_picks_a_lower_named_readme_md_after_the_manifest() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
            ("package/README.md", "# Lower"),
            ("package/README.MD", "# Upper"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Upper");
}

#[test]
fn publish_manifest_ignores_non_file_readme_entries() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball_with_non_files(&dir);
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Bare");
}

#[test]
fn publish_manifest_keeps_a_readme_already_in_the_manifest() {
    for readme_entry in ["package/README.md", "package/README"] {
        let dir = TempDir::new().unwrap();
        let path = write_tarball(
            &dir,
            &[
                ("package/package.json", r#"{"name":"foo","version":"1.0.0","readme":"embedded"}"#),
                (readme_entry, "# Hello"),
            ],
        );
        let manifest = extract_publish_manifest_from_packed(&path).unwrap();
        assert_eq!(manifest["readme"], "embedded", "{readme_entry}");
    }
}

#[test]
fn publish_manifest_replaces_invalid_utf8_in_the_readme() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball_bytes(
        &dir,
        &[
            ("package/package.json", br#"{"name":"foo","version":"1.0.0"}"#),
            ("package/README", b"text\xff"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "text\u{FFFD}");
}

#[test]
fn publish_manifest_picks_the_lowest_name_among_markdown_readmes() {
    let dir = TempDir::new().unwrap();
    let path = write_tarball(
        &dir,
        &[
            ("package/README.mdown", "# Mdown"),
            ("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#),
            ("package/README.markdown", "# Markdown"),
        ],
    );
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert_eq!(manifest["readme"], "# Markdown");
}

#[test]
fn publish_manifest_leaves_readme_unset_without_a_tarball_readme() {
    let dir = TempDir::new().unwrap();
    let path =
        write_tarball(&dir, &[("package/package.json", r#"{"name":"foo","version":"1.0.0"}"#)]);
    let manifest = extract_publish_manifest_from_packed(&path).unwrap();
    assert!(manifest.get("readme").is_none());
}

#[test]
fn rejects_oversized_tar_metadata_before_reading_payload() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("metadata.tgz");
    let mut header = tar::Header::new_gnu();
    header.set_path("metadata").unwrap();
    header.set_entry_type(tar::EntryType::XHeader);
    header.set_size(pnpm_tarball::MAX_TARBALL_METADATA_BYTES + 1);
    header.set_cksum();
    let file = std::fs::File::create(&path).unwrap();
    let mut gzip = GzEncoder::new(file, Compression::default());
    gzip.write_all(header.as_bytes()).unwrap();
    gzip.finish().unwrap();
    let path = path.to_str().unwrap();
    for error in [
        extract_manifest_from_packed(path).unwrap_err(),
        extract_publish_manifest_from_packed(path).unwrap_err(),
    ] {
        match error {
            ExtractManifestError::Read { source, .. } => {
                assert!(source.to_string().contains("exceeds the"));
            }
            error => panic!("expected metadata limit error, got {error:?}"),
        }
    }
}

#[test]
fn rejects_oversized_buffered_entries_before_reading_payload() {
    let directory = TempDir::new().unwrap();
    for name in ["package/package.json", "package/README.md"] {
        let path = directory.path().join("buffered.tgz");
        let mut header = tar::Header::new_gnu();
        header.set_path(name).unwrap();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(pnpm_tarball::MAX_TARBALL_METADATA_BYTES + 1);
        header.set_cksum();
        let file = std::fs::File::create(&path).unwrap();
        let mut gzip = GzEncoder::new(file, Compression::default());
        gzip.write_all(header.as_bytes()).unwrap();
        gzip.finish().unwrap();
        let path = path.to_str().unwrap();
        let error = extract_publish_manifest_from_packed(path).unwrap_err();
        match error {
            ExtractManifestError::Read { source, .. } => {
                assert!(source.to_string().contains("buffered entry limit"));
                assert!(source.to_string().contains(name));
            }
            error => panic!("expected buffered entry limit error, got {error:?}"),
        }
        if name.ends_with("package.json") {
            let error = extract_manifest_from_packed(path).unwrap_err();
            assert!(error.to_string().contains("buffered entry limit"));
        }
    }
}
