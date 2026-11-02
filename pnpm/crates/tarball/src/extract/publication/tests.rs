use super::{PublicationArchiveError, read_publication_archive};
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;

fn tar_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        // `append_data` refuses `..`, so the path goes into the raw header.
        header.as_gnu_mut().unwrap().name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_cksum();
        builder.append(&header, *data).unwrap();
    }
    builder.into_inner().unwrap()
}

fn symlink_tar(path: &str, target: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    builder.append_link(&mut header, path, target).unwrap();
    builder.into_inner().unwrap()
}

#[test]
fn reads_the_manifest_shrinkwrap_and_files() {
    let manifest = br#"{"name":"widgets","version":"1.0.0"}"#;
    let tar = tar_of(&[
        ("package/package.json", manifest),
        ("package/npm-shrinkwrap.json", b"{}"),
        ("package/binding.gyp", b"{}"),
        ("package/lib/index.js", b""),
    ]);
    let archive = read_publication_archive(tar.as_slice()).unwrap();
    assert_eq!(archive.manifest, manifest);
    assert!(archive.manifest_integrity.to_string().starts_with("sha512-"));
    assert!(archive.shrinkwrap_integrity.is_some());
    assert!(archive.binding_gyp);
    let expected: BTreeSet<String> =
        ["binding.gyp", "lib/index.js", "npm-shrinkwrap.json", "package.json"]
            .into_iter()
            .map(str::to_string)
            .collect();
    assert_eq!(archive.files, expected);
}

#[test]
fn a_nested_binding_gyp_does_not_count() {
    let tar = tar_of(&[("package/package.json", b"{}"), ("package/src/binding.gyp", b"{}")]);
    let archive = read_publication_archive(tar.as_slice()).unwrap();
    assert!(!archive.binding_gyp);
    assert_eq!(archive.shrinkwrap_integrity, None);
}

#[test]
fn refuses_two_entries_that_extract_to_the_manifest() {
    let tar = tar_of(&[("package/package.json", b"{}"), ("other/package.json", b"{}")]);
    let error = read_publication_archive(tar.as_slice()).unwrap_err();
    assert!(matches!(error, PublicationArchiveError::AmbiguousManifest(_, "package.json")));
}

#[test]
fn refuses_a_manifest_spelled_in_another_case() {
    let tar = tar_of(&[("package/package.json", b"{}"), ("package/Package.json", b"{}")]);
    let error = read_publication_archive(tar.as_slice()).unwrap_err();
    assert!(matches!(error, PublicationArchiveError::AmbiguousManifest(..)), "{error}");
}

#[test]
fn refuses_a_manifest_that_is_a_link() {
    let tar = symlink_tar("package/package.json", "/etc/passwd");
    let error = read_publication_archive(tar.as_slice()).unwrap_err();
    assert!(matches!(error, PublicationArchiveError::AmbiguousManifest(..)), "{error}");
}

#[test]
fn refuses_an_entry_that_escapes_the_root() {
    let tar = tar_of(&[("package/package.json", b"{}"), ("package/../../evil.js", b"")]);
    let error = read_publication_archive(tar.as_slice()).unwrap_err();
    assert!(matches!(error, PublicationArchiveError::UnsafePath(_)), "{error}");
}

#[test]
fn refuses_an_archive_without_a_manifest() {
    let tar = tar_of(&[("package/index.js", b"")]);
    let error = read_publication_archive(tar.as_slice()).unwrap_err();
    assert!(matches!(error, PublicationArchiveError::MissingManifest));
}

#[test]
fn refuses_bytes_that_are_not_a_tar() {
    let error = read_publication_archive(&b"not a tar archive at all"[..]).unwrap_err();
    assert!(
        matches!(
            error,
            PublicationArchiveError::Unreadable(_) | PublicationArchiveError::MissingManifest
        ),
        "{error}",
    );
}
