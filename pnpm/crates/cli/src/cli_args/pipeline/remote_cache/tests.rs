use super::{ArtifactSigner, pack, team_query, unpack};
use crate::cli_args::pipeline::cache::TaskCache;
use flate2::{Compression, write::GzEncoder};
use std::fs;

const KEY: &str = "abcdef";

fn stored_cache() -> (tempfile::TempDir, tempfile::TempDir, TaskCache) {
    let project = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let cache = TaskCache::open(storage.path(), project.path()).unwrap();
    fs::create_dir_all(project.path().join("out/nested")).unwrap();
    fs::write(project.path().join("out/result"), "built").unwrap();
    fs::write(project.path().join("out/nested/deep"), "deep").unwrap();
    cache
        .store(KEY, project.path(), "build", &["out/**".to_string()], Vec::new())
        .unwrap();
    (project, storage, cache)
}

/// A gzipped tar with one entry whose name is written into the header
/// verbatim, bypassing the checks `tar::Builder` applies to the paths it
/// is given.
fn raw_archive(name: &str, entry_type: tar::EntryType, link: Option<&str>) -> Vec<u8> {
    let mut header = tar::Header::new_gnu();
    header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
    header.set_entry_type(entry_type);
    header.set_size(if entry_type == tar::EntryType::Regular { 4 } else { 0 });
    header.set_mode(0o644);
    if let Some(link) = link {
        header.as_gnu_mut().unwrap().linkname[..link.len()].copy_from_slice(link.as_bytes());
    }
    header.set_cksum();
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let data: &[u8] = if entry_type == tar::EntryType::Regular { b"evil" } else { b"" };
    builder.append(&header, data).unwrap();
    builder
        .into_inner()
        .unwrap()
        .finish()
        .unwrap()
}

#[test]
fn a_packed_entry_unpacks_into_an_equivalent_entry() {
    let (_project, _storage, cache) = stored_cache();
    let archive = pack(&cache.lookup(KEY).unwrap()).unwrap();

    let other_project = tempfile::tempdir().unwrap();
    let other_storage = tempfile::tempdir().unwrap();
    let other = TaskCache::open(other_storage.path(), other_project.path()).unwrap();
    other.import(KEY, |staging| unpack(&archive, staging)).unwrap();

    let original = cache.lookup(KEY).unwrap();
    let imported = other.lookup(KEY).unwrap();
    assert_eq!(imported.files, original.files);
    assert_eq!(imported.hashes, original.hashes);
    other.restore(&imported, other_project.path(), "build").unwrap();
    assert_eq!(fs::read_to_string(other_project.path().join("out/nested/deep")).unwrap(), "deep");
}

#[test]
fn entries_outside_meta_and_outputs_are_refused() {
    for (name, entry_type, link) in [
        ("../escape", tar::EntryType::Regular, None),
        ("outputs/../../escape", tar::EntryType::Regular, None),
        ("/escape", tar::EntryType::Regular, None),
        ("node_modules/escape", tar::EntryType::Regular, None),
        ("meta.json/escape", tar::EntryType::Regular, None),
        ("outputs/link", tar::EntryType::Symlink, Some("/etc/passwd")),
        ("outputs/hard", tar::EntryType::Link, Some("meta.json")),
    ] {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let result = unpack(&raw_archive(name, entry_type, link), &staging);
        assert!(result.is_err(), "{name} must be refused");
        assert!(!root.path().join("escape").exists(), "{name} escaped the staging directory");
        assert!(!staging.join("outputs/link").exists());
    }
}

#[test]
fn an_import_keeps_the_entry_already_stored() {
    let (_project, _storage, cache) = stored_cache();
    cache
        .import(KEY, |staging| fs::write(staging.join("meta.json"), "{}"))
        .unwrap();
    assert_eq!(cache.lookup(KEY).unwrap().files, ["out/nested/deep", "out/result"]);
}

#[test]
fn a_signature_covers_the_key_the_team_and_the_body() {
    let signer =
        |key: &[u8], team: &str| ArtifactSigner { key: key.to_vec(), team: team.to_string() };
    let tag = signer(b"secret", "team_one").sign(KEY, b"body");
    signer(b"secret", "team_one")
        .verify(KEY, b"body", Some(&tag))
        .unwrap();

    for (verifier, cache_key, body, tag) in [
        (signer(b"secret", "team_one"), KEY, &b"tampered"[..], Some(tag.as_str())),
        (signer(b"secret", "team_one"), "abcdeg", b"body", Some(&tag)),
        (signer(b"secret", "team_one"), KEY, b"body", None),
        (signer(b"secret", "team_one"), KEY, b"body", Some("not base64!")),
        (signer(b"secret", "team_two"), KEY, b"body", Some(&tag)),
        (signer(b"other", "team_one"), KEY, b"body", Some(&tag)),
    ] {
        let result = verifier.verify(cache_key, body, tag);
        assert!(result.is_err(), "{cache_key} {body:?} {tag:?} must be rejected");
    }
}

#[test]
fn a_team_id_and_a_team_slug_use_different_parameters() {
    assert_eq!(team_query(""), "");
    assert_eq!(team_query("team_abc"), "?teamId=team_abc");
    assert_eq!(team_query("my team"), "?slug=my+team");
}

#[test]
fn a_cleartext_server_is_refused() {
    let config = pnpm_config::Config {
        pipeline_remote_cache: Some(pnpm_config::PipelineRemoteCacheSettings {
            url: Some("http://cache.example.com".to_string()),
            signature_key: Some("secret".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let error = super::RemoteTaskCache::open(&config).err().expect("the server is refused");
    assert!(error.contains("neither HTTPS nor a loopback address"), "{error}");
}
