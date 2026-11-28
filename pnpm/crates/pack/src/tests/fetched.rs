use super::{
    Host,
    PackError,
};
use crate::{
    FetchedPackOptions,
    pack_fetched_tarball,
};
use flate2::{
    Compression,
    write::GzEncoder,
};
use serde_json::{
    Value,
    json,
};
use std::{
    io::Write,
    path::Path,
};
use tempfile::tempdir;

fn append_entry(builder: &mut tar::Builder<impl Write>, name: &str, data: &[u8]) {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    builder.append_data(&mut header, name, data).unwrap();
}

fn tar_bytes(files: &[(&str, &str)]) -> Vec<u8> {
    let mut raw = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut raw);
        for (name, contents) in files {
            append_entry(&mut builder, name, contents.as_bytes());
        }
        builder.finish().unwrap();
    }
    raw
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn minimal_tgz(manifest: &Value, files: &[(&str, &str)]) -> Vec<u8> {
    let manifest_json = serde_json::to_string_pretty(manifest).unwrap();
    let mut entries: Vec<(String, &str)> =
        vec![("package/package.json".to_string(), &manifest_json)];
    entries.extend(
        files
            .iter()
            .map(|(name, contents)| (format!("package/{name}"), *contents)),
    );
    let refs: Vec<(&str, &str)> = entries
        .iter()
        .map(|(name, contents)| (name.as_str(), *contents))
        .collect();
    gzip(&tar_bytes(&refs))
}

fn fetched<'a>(dir: &'a Path, bytes: &'a [u8]) -> FetchedPackOptions<'a> {
    FetchedPackOptions {
        bytes,
        source: "https://registry.example.test/foo/-/foo-1.0.0.tgz",
        expected_integrity: None,
        base_dir: dir,
        dry_run: false,
        out: None,
        destination: None,
    }
}

#[test]
fn packs_a_fetched_tarball_under_the_manifest_name_and_version() {
    let dir = tempdir().unwrap();
    let manifest = json!({ "name": "foo", "version": "1.0.0" });
    let bytes = minimal_tgz(&manifest, &[("dist/index.js", "console.log()\n")]);

    let result = pack_fetched_tarball::<Host>(&fetched(dir.path(), &bytes)).unwrap();

    let written = dir.path().join("foo-1.0.0.tgz");
    assert!(written.exists(), "tarball written to the base directory");
    assert_eq!(std::fs::read(&written).unwrap(), bytes, "bytes written through unchanged");
    assert_eq!(result.tarball_path, "foo-1.0.0.tgz");
    assert_eq!(result.contents, vec!["dist/index.js".to_string(), "package.json".to_string()]);
    assert_eq!(result.packed_manifest, manifest);
    let archive = result.archive.expect("a written tarball has a digest");
    assert_eq!(archive.size, bytes.len() as u64);
    assert!(
        result.unpacked_size > 0,
        "unpacked size sums the archive's entries: {}",
        result.unpacked_size
    );
}

#[test]
fn checks_the_registry_declared_integrity() {
    let dir = tempdir().unwrap();
    let bytes = minimal_tgz(&json!({ "name": "foo", "version": "1.0.0" }), &[]);
    let matching = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&bytes)
        .result();
    let mismatched: ssri::Integrity =
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            .parse()
            .unwrap();

    let mut opts = fetched(dir.path(), &bytes);
    opts.expected_integrity = Some(&matching);
    pack_fetched_tarball::<Host>(&opts).unwrap();

    let mismatch_dir = tempdir().unwrap();
    let mut opts = fetched(mismatch_dir.path(), &bytes);
    opts.expected_integrity = Some(&mismatched);
    let error = pack_fetched_tarball::<Host>(&opts).unwrap_err();
    assert!(
        matches!(error, PackError::FetchedIntegrityMismatch { .. }),
        "mismatched bytes fail the integrity check: {error}"
    );
    assert!(
        !mismatch_dir
            .path()
            .join("foo-1.0.0.tgz")
            .exists(),
        "nothing written on mismatch"
    );
}

#[test]
fn rejects_an_archive_without_a_manifest() {
    let dir = tempdir().unwrap();
    let bytes = gzip(&tar_bytes(&[("package/index.js", "console.log()\n")]));
    let error = pack_fetched_tarball::<Host>(&fetched(dir.path(), &bytes)).unwrap_err();
    assert!(
        matches!(error, PackError::FetchedTarballUnreadable { .. }),
        "a manifestless archive is unreadable: {error}"
    );
}

#[test]
fn dry_run_writes_nothing_and_reports_no_digest() {
    let dir = tempdir().unwrap();
    let bytes = minimal_tgz(&json!({ "name": "foo", "version": "1.0.0" }), &[]);
    let mut opts = fetched(dir.path(), &bytes);
    opts.dry_run = true;

    let result = pack_fetched_tarball::<Host>(&opts).unwrap();

    assert!(result.archive.is_none(), "a dry run writes no archive");
    assert!(
        !dir.path()
            .join("foo-1.0.0.tgz")
            .exists(),
        "a dry run writes no file"
    );
}

#[test]
fn honors_the_out_template_and_pack_destination() {
    let dir = tempdir().unwrap();
    let bytes = minimal_tgz(&json!({ "name": "@scope/foo", "version": "2.0.0" }), &[]);
    let mut opts = fetched(dir.path(), &bytes);
    opts.out = Some("dist/%s-%v.tgz");

    let result = pack_fetched_tarball::<Host>(&opts).unwrap();

    assert!(
        dir.path()
            .join("dist/scope-foo-2.0.0.tgz")
            .exists(),
        "out template expanded: {result:?}"
    );

    let dir2 = tempdir().unwrap();
    let mut opts = fetched(dir2.path(), &bytes);
    opts.destination = Some("archives");
    pack_fetched_tarball::<Host>(&opts).unwrap();
    assert!(
        dir2.path()
            .join("archives/scope-foo-2.0.0.tgz")
            .exists(),
        "destination honored"
    );
}

#[test]
fn accepts_a_sha1_declared_integrity() {
    let dir = tempdir().unwrap();
    let bytes = minimal_tgz(&json!({ "name": "foo", "version": "1.0.0" }), &[]);
    let sha1 = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha1)
        .chain(&bytes)
        .result();
    let mut opts = fetched(dir.path(), &bytes);
    opts.expected_integrity = Some(&sha1);

    pack_fetched_tarball::<Host>(&opts)
        .expect("a registry entry declaring only SHA-1 still verifies");
    assert!(
        dir.path()
            .join("foo-1.0.0.tgz")
            .exists()
    );
}

#[test]
fn rejects_a_manifest_identity_that_escapes_the_destination() {
    let dir = tempdir().unwrap();
    for manifest in [
        json!({ "name": "foo", "version": "../backup" }),
        json!({ "name": "../evil", "version": "1.0.0" }),
    ] {
        let bytes = minimal_tgz(&manifest, &[]);
        let error = pack_fetched_tarball::<Host>(&fetched(dir.path(), &bytes)).unwrap_err();
        assert!(
            matches!(
                error,
                PackError::InvalidPackageVersion { .. } | PackError::InvalidPackageName { .. }
            ),
            "manifest {manifest} is rejected: {error}"
        );
    }
    assert!(
        !dir.path()
            .parent()
            .unwrap()
            .join("backup.tgz")
            .exists(),
        "nothing written outside the destination"
    );
}

#[test]
fn packs_a_plain_tar() {
    let dir = tempdir().unwrap();
    let manifest = json!({ "name": "foo", "version": "1.0.0" });
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
    let bytes = tar_bytes(&[("package/package.json", manifest_json.as_str())]);

    let result = pack_fetched_tarball::<Host>(&fetched(dir.path(), &bytes)).unwrap();

    assert_eq!(result.tarball_path, "foo-1.0.0.tgz");
    assert!(
        dir.path()
            .join("foo-1.0.0.tgz")
            .exists()
    );
}
