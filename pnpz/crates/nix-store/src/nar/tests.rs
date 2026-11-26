use super::{
    HashingReader,
    UnpackError,
    pack,
    unpack,
};
use pretty_assertions::assert_eq;
use std::{
    fs,
    os::unix::fs::{
        MetadataExt,
        PermissionsExt,
        symlink,
    },
    path::Path,
};
use tempfile::TempDir;

/// Encodes NAR strings, for building archives a well-behaved packer never
/// would.
fn nar(strings: &[&str]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for string in strings {
        bytes.extend((string.len() as u64).to_le_bytes());
        bytes.extend(string.as_bytes());
        bytes.resize(bytes.len().next_multiple_of(8), 0);
    }
    bytes
}

fn tree(root: &Path) {
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("share/empty")).unwrap();
    fs::write(root.join("bin/tool"), "#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(root.join("bin/tool"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(root.join("share/data"), "12345678").unwrap();
    fs::write(root.join("share/zero"), "").unwrap();
    symlink("../bin/tool", root.join("share/link")).unwrap();
}

#[test]
fn unpacks_what_pack_wrote() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    tree(&source);
    let mut archive = Vec::new();
    pack(&source, &mut archive).unwrap();

    let unpacked = temp.path().join("unpacked");
    unpack(archive.as_slice(), &unpacked).unwrap();

    assert_eq!(fs::read_to_string(unpacked.join("bin/tool")).unwrap(), "#!/bin/sh\necho hi\n");
    assert_eq!(fs::read_to_string(unpacked.join("share/data")).unwrap(), "12345678");
    assert_eq!(fs::read(unpacked.join("share/zero")).unwrap(), b"");
    assert_eq!(fs::read_link(unpacked.join("share/link")).unwrap(), Path::new("../bin/tool"));
    assert!(unpacked.join("share/empty").is_dir());

    let mut repacked = Vec::new();
    pack(&unpacked, &mut repacked).unwrap();
    assert_eq!(repacked, archive);
}

#[test]
fn gives_the_tree_store_permissions_and_timestamps() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    tree(&source);
    let mut archive = Vec::new();
    pack(&source, &mut archive).unwrap();
    let unpacked = temp.path().join("unpacked");
    unpack(archive.as_slice(), &unpacked).unwrap();

    let stat = |path: &str| fs::metadata(unpacked.join(path)).unwrap();
    assert_eq!(stat("bin/tool").mode() & 0o7777, 0o555);
    assert_eq!(stat("share/data").mode() & 0o7777, 0o444);
    assert_eq!(stat("share").mode() & 0o7777, 0o555);
    assert_eq!(stat("").mode() & 0o7777, 0o555);
    assert_eq!(stat("bin/tool").mtime(), 1);
    assert_eq!(stat("share").mtime(), 1);
}

#[test]
fn unpacks_a_single_file() {
    let temp = TempDir::new().unwrap();
    let archive = nar(&["nix-archive-1", "(", "type", "regular", "contents", "hello", ")"]);
    let unpacked = temp.path().join("file");
    unpack(archive.as_slice(), &unpacked).unwrap();
    assert_eq!(fs::read_to_string(unpacked).unwrap(), "hello");
}

#[test]
fn hashes_the_archive_while_unpacking() {
    let temp = TempDir::new().unwrap();
    let archive = nar(&["nix-archive-1", "(", "type", "regular", "contents", "", ")"]);
    let mut reader = HashingReader::new(archive.as_slice());
    unpack(&mut reader, &temp.path().join("file")).unwrap();
    let (hash, size) = reader.finish().unwrap();
    assert_eq!(size, archive.len() as u64);
    // `nix hash file --type sha256 --base32` of an empty file's NAR.
    assert_eq!(hash.to_nix_base32(), "sha256:0ip26j2h11n1kgkz36rl4akv694yz65hr72q4kv4b3lxcbi65b3p");
}

#[test]
fn trailing_bytes_change_the_hash() {
    let temp = TempDir::new().unwrap();
    let mut archive = nar(&["nix-archive-1", "(", "type", "regular", "contents", "", ")"]);
    archive.extend(b"trailing");
    let mut reader = HashingReader::new(archive.as_slice());
    unpack(&mut reader, &temp.path().join("file")).unwrap();
    let (hash, size) = reader.finish().unwrap();
    assert_eq!(size, archive.len() as u64);
    assert_ne!(hash.to_nix_base32(), "sha256:0ip26j2h11n1kgkz36rl4akv694yz65hr72q4kv4b3lxcbi65b3p");
}

fn directory_with_entry(name: &str) -> Vec<u8> {
    let entry = ["entry", "(", "name", name, "node"];
    let file = ["(", "type", "regular", "contents", "x", ")", ")"];
    nar(&[&["nix-archive-1", "(", "type", "directory"], &entry[..], &file[..], &[")"]].concat())
}

#[test]
fn rejects_entry_names_that_leave_the_destination() {
    for name in ["..", ".", "", "a/b", "../escape"] {
        let temp = TempDir::new().unwrap();
        let error = unpack(directory_with_entry(name).as_slice(), &temp.path().join("out"));
        assert!(matches!(error, Err(UnpackError::Malformed(_))), "{name:?}: {error:?}");
        assert!(!temp.path().join("escape").exists());
    }
}

#[test]
fn rejects_unsorted_and_duplicate_entries() {
    let file = ["node", "(", "type", "regular", "contents", "x", ")", ")"];
    for names in [["b", "a"], ["a", "a"]] {
        let first = [&["entry", "(", "name", names[0]], &file[..]].concat();
        let second = [&["entry", "(", "name", names[1]], &file[..]].concat();
        let archive =
            nar(&[&["nix-archive-1", "(", "type", "directory"], &first[..], &second[..], &[")"]]
                .concat());
        let temp = TempDir::new().unwrap();
        let error = unpack(archive.as_slice(), &temp.path().join("out"));
        assert!(matches!(error, Err(UnpackError::Malformed(_))), "{names:?}: {error:?}");
    }
}

#[test]
fn rejects_a_truncated_archive() {
    let temp = TempDir::new().unwrap();
    let archive = nar(&["nix-archive-1", "(", "type", "regular", "contents", "hello", ")"]);
    let error = unpack(&archive[..archive.len() - 20], &temp.path().join("file"));
    assert!(matches!(error, Err(UnpackError::Read(_))), "{error:?}");
}

#[test]
fn rejects_another_format() {
    let temp = TempDir::new().unwrap();
    let error = unpack(nar(&["tar"]).as_slice(), &temp.path().join("file"));
    assert!(matches!(error, Err(UnpackError::Malformed(_))), "{error:?}");
}

#[test]
fn refuses_to_overwrite_the_destination() {
    let temp = TempDir::new().unwrap();
    let archive = nar(&["nix-archive-1", "(", "type", "regular", "contents", "hello", ")"]);
    fs::write(temp.path().join("file"), "kept").unwrap();
    let error = unpack(archive.as_slice(), &temp.path().join("file"));
    assert!(matches!(error, Err(UnpackError::Write { .. })), "{error:?}");
    assert_eq!(fs::read_to_string(temp.path().join("file")).unwrap(), "kept");
}
