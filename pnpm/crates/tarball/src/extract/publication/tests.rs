use super::{PublicationArchiveError, read_publication_archive, read_publication_archive_within};
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
    assert_eq!(archive.manifest_path, "package/package.json");
    assert!(archive.shrinkwrap_integrity.is_some());
    assert_eq!(archive.shrinkwrap_path.as_deref(), Some("package/npm-shrinkwrap.json"));
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
    assert_eq!(archive.shrinkwrap_path, None);
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
fn refuses_an_entry_that_escapes_the_root_through_backslashes() {
    let tar = tar_of(&[("package/package.json", b"{}"), (r"package/..\..\evil.js", b"")]);
    let error = read_publication_archive(tar.as_slice()).unwrap_err();
    assert!(matches!(error, PublicationArchiveError::UnsafePath(_)), "{error}");
}

#[test]
fn refuses_a_link_or_directory_that_escapes_the_root() {
    for entry_type in [tar::EntryType::Symlink, tar::EntryType::Directory] {
        let mut builder = tar::Builder::new(Vec::new());
        let mut manifest = tar::Header::new_gnu();
        manifest.set_size(2);
        manifest.set_mode(0o644);
        builder
            .append_data(&mut manifest, "package/package.json", &b"{}"[..])
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(entry_type);
        header.set_size(0);
        let path = b"package/../../outside";
        header.as_gnu_mut().unwrap().name[..path.len()].copy_from_slice(path);
        header.set_cksum();
        builder
            .append(&header, &[][..])
            .unwrap();
        let tar = builder.into_inner().unwrap();
        let error = read_publication_archive(tar.as_slice()).unwrap_err();
        assert!(matches!(error, PublicationArchiveError::UnsafePath(_)), "{entry_type:?}: {error}");
    }
}

#[test]
fn accepts_a_root_directory_entry() {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_size(0);
    builder
        .append_data(&mut header, "./", &[][..])
        .unwrap();
    let mut manifest = tar::Header::new_gnu();
    manifest.set_size(2);
    manifest.set_mode(0o644);
    builder
        .append_data(&mut manifest, "package/package.json", &b"{}"[..])
        .unwrap();
    read_publication_archive(builder.into_inner().unwrap().as_slice()).unwrap();
}

fn global_header_tar(records: &[u8]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut global = tar::Header::new_ustar();
    global.set_entry_type(tar::EntryType::XGlobalHeader);
    global.set_size(records.len() as u64);
    global.set_mode(0o644);
    builder.append_data(&mut global, "pax_global_header", records).unwrap();
    let mut manifest = tar::Header::new_ustar();
    manifest.set_size(2);
    manifest.set_mode(0o644);
    builder
        .append_data(&mut manifest, "package/package.json", &b"{}"[..])
        .unwrap();
    builder.into_inner().unwrap()
}

#[test]
fn refuses_a_global_header_that_overrides_entry_sizes_or_paths() {
    for records in [&b"11 size=10\n"[..], b"21 path=package/x.js\n", b"17 linkpath=/etc\n"] {
        let tar = global_header_tar(records);
        let error = read_publication_archive(tar.as_slice()).unwrap_err();
        assert!(
            matches!(error, PublicationArchiveError::GlobalHeaderOverride(_)),
            "{}: {error}",
            String::from_utf8_lossy(records),
        );
    }
}

#[test]
fn accepts_the_comment_git_archive_writes_in_a_global_header() {
    let tar = global_header_tar(b"52 comment=0123456789abcdef0123456789abcdef01234567\n");
    read_publication_archive(tar.as_slice()).unwrap();
}

#[test]
fn refuses_data_after_the_end_of_archive_marker() {
    let mut tar = tar_of(&[("package/package.json", b"{}")]);
    tar.extend_from_slice(b"trailing garbage");
    let error = read_publication_archive(tar.as_slice()).unwrap_err();
    assert!(matches!(error, PublicationArchiveError::Unreadable(_)), "{error}");
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

#[test]
fn refuses_file_paths_longer_in_total_than_the_limit() {
    let tar = tar_of(&[("package/package.json", b"{}"), ("package/lib/a.js", b"")]);
    // "package.json" and "lib/a.js": 20 bytes.
    read_publication_archive_within(tar.as_slice(), 20).unwrap();
    assert!(matches!(
        read_publication_archive_within(tar.as_slice(), 19),
        Err(PublicationArchiveError::OversizedPaths),
    ));
}
