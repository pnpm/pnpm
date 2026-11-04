use super::{differs_only_in_registry_metadata, take_rehashed_integrity};
use pnpm_lockfile::Lockfile;
use ssri::Integrity;
use std::path::Path;

const LOCKFILE: &str = "\
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      dep:
        specifier: 1.0.0
        version: 1.0.0
packages:
  dep@1.0.0:
    resolution: {integrity: sha512-dGVzdA==}
snapshots:
  dep@1.0.0: {}
";

const SHA1_LOCKFILE: &str = "\
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      dep:
        specifier: 1.0.0
        version: 1.0.0
packages:
  dep@1.0.0:
    resolution: {integrity: sha1-dGVzdA==}
snapshots:
  dep@1.0.0: {}
";

fn parse(text: &str) -> Lockfile {
    Lockfile::parse(text, Path::new("pnpm-lock.yaml"))
        .expect("parse lockfile")
        .expect("lockfile is not empty")
}

fn canonical() -> String {
    parse(LOCKFILE).to_yaml_string().expect("serialize lockfile")
}

fn deprecated() -> String {
    let mut lockfile = parse(LOCKFILE);
    for metadata in lockfile.packages.iter_mut().flat_map(|packages| packages.values_mut()) {
        metadata.deprecated = Some("use something else".to_string());
    }
    lockfile.to_yaml_string().expect("serialize lockfile")
}

fn only_registry_metadata_differs(previous: &str, next: &str) -> bool {
    eprintln!("PREVIOUS:\n{previous}\nNEXT:\n{next}");
    differs_only_in_registry_metadata(
        (Some(previous), Some(&parse(previous))),
        (Some(next), Some(&parse(next))),
    )
}

#[test]
fn a_new_deprecation_notice_alone_is_accepted() {
    assert!(only_registry_metadata_differs(&canonical(), &deprecated()));
}

#[test]
fn a_sha1_integrity_replaced_by_a_sha512_one_is_accepted() {
    let previous = parse(SHA1_LOCKFILE).to_yaml_string().expect("serialize lockfile");
    assert!(only_registry_metadata_differs(&previous, &canonical()));
}

#[test]
fn a_sha512_integrity_replaced_by_a_sha1_one_is_rejected() {
    let next = parse(SHA1_LOCKFILE).to_yaml_string().expect("serialize lockfile");
    assert!(!only_registry_metadata_differs(&canonical(), &next));
}

#[test]
fn a_different_digest_of_the_same_algorithm_is_rejected() {
    let next = canonical().replace("sha512-dGVzdA==", "sha512-b3RoZXI=");
    assert_ne!(next, canonical(), "the digest must change");
    assert!(!only_registry_metadata_differs(&canonical(), &next));
}

#[test]
fn an_integrity_change_with_a_new_revision_is_rejected() {
    let previous = parse(SHA1_LOCKFILE).to_yaml_string().expect("serialize lockfile");
    let next = canonical()
        .replace("integrity: sha512-dGVzdA==}", "integrity: sha512-dGVzdA==, revision: 1}");
    assert_ne!(next, canonical(), "the revision must be recorded");
    assert!(!only_registry_metadata_differs(&previous, &next));
}

fn tarball(resolution: &str) -> String {
    parse(&LOCKFILE.replace("{integrity: sha512-dGVzdA==}", resolution))
        .to_yaml_string()
        .expect("serialize lockfile")
}

#[test]
fn a_tarball_integrity_replaced_by_another_algorithm_is_accepted() {
    let previous = tarball("{integrity: sha1-dGVzdA==, tarball: https://example.com/dep.tgz}");
    let next = tarball("{integrity: sha512-dGVzdA==, tarball: https://example.com/dep.tgz}");
    assert!(only_registry_metadata_differs(&previous, &next));
}

#[test]
fn a_tarball_integrity_change_with_a_new_url_is_rejected() {
    let previous = tarball("{integrity: sha1-dGVzdA==, tarball: https://example.com/dep.tgz}");
    let next = tarball("{integrity: sha512-dGVzdA==, tarball: https://example.com/other.tgz}");
    assert!(!only_registry_metadata_differs(&previous, &next));
}

#[test]
fn a_tarball_integrity_the_lockfile_did_not_record_is_rejected() {
    let previous = tarball("{tarball: https://example.com/dep.tgz}");
    let next = tarball("{integrity: sha512-dGVzdA==, tarball: https://example.com/dep.tgz}");
    assert!(!only_registry_metadata_differs(&previous, &next));
}

#[test]
fn an_integrity_change_with_a_new_resolution_kind_is_rejected() {
    let previous = parse(SHA1_LOCKFILE).to_yaml_string().expect("serialize lockfile");
    let next = tarball("{integrity: sha512-dGVzdA==, tarball: https://example.com/dep.tgz}");
    assert!(!only_registry_metadata_differs(&previous, &next));
}

#[test]
fn a_rewrite_of_a_lockfile_without_packages_is_rejected() {
    let previous = parse("lockfileVersion: '9.0'\nimporters:\n  .: {}\n")
        .to_yaml_string()
        .expect("serialize lockfile");
    assert!(!only_registry_metadata_differs(&previous, &canonical()));
}

#[test]
fn an_integrity_without_any_digest_is_not_taken() {
    let mut integrity: Integrity = "sha1-dGVzdA==".parse().expect("parse integrity");
    let locked = integrity.clone();
    take_rehashed_integrity(&mut integrity, &Integrity { hashes: Vec::new() });
    assert_eq!(integrity, locked);
}

#[test]
fn a_line_ending_rewrite_is_rejected() {
    let previous = canonical().replace('\n', "\r\n");
    assert!(!only_registry_metadata_differs(&previous, &deprecated()));
}

#[test]
fn a_byte_order_mark_removal_is_rejected() {
    let previous = format!("\u{feff}{}", canonical());
    assert!(!only_registry_metadata_differs(&previous, &deprecated()));
}

#[test]
fn a_reformatting_rewrite_is_rejected() {
    let previous = canonical().replace("\n\n", "\n");
    assert_ne!(previous, canonical(), "the canonical form must separate sections");
    assert!(!only_registry_metadata_differs(&previous, &deprecated()));
}
