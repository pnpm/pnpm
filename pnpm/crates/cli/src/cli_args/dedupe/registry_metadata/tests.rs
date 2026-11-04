use super::differs_only_in_registry_metadata;
use pnpm_lockfile::Lockfile;
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
