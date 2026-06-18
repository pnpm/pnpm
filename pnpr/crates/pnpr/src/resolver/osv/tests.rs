use std::{fs, sync::Arc};

use pacquet_resolving_resolver_base::{PackageVersionGuard, PackageVersionGuardDecision};

use super::OsvIndex;

#[test]
fn loads_directory_and_matches_semver_ranges() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-test.json"),
        r#"{
          "id": "GHSA-test",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "acme" },
            "ranges": [{
              "type": "SEMVER",
              "events": [
                { "introduced": "1.0.0" },
                { "fixed": "1.2.0" }
              ]
            }]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    assert_eq!(index.vulnerability_ids("acme", "0.9.0"), Vec::<String>::new());
    assert_eq!(index.vulnerability_ids("acme", "1.1.0"), vec!["GHSA-test"]);
    assert_eq!(index.vulnerability_ids("acme", "1.2.0"), Vec::<String>::new());
}

#[test]
fn explicit_versions_match_even_without_semver() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-exact.json"),
        r#"{
          "id": "GHSA-exact",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "odd" },
            "versions": ["2026.06.18-custom"]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    assert_eq!(index.vulnerability_ids("odd", "2026.06.18-custom"), vec!["GHSA-exact"]);
}

#[tokio::test]
async fn package_version_guard_rejects_vulnerable_versions() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-guard.json"),
        r#"{
          "id": "GHSA-guard",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "guarded" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write record");

    let index = Arc::new(OsvIndex::load_from_path(dir.path()).expect("load index"));

    assert_eq!(index.check("guarded", "1.1.0").await.unwrap(), PackageVersionGuardDecision::Allow);
    match index.check("guarded", "1.0.0").await.unwrap() {
        PackageVersionGuardDecision::Reject { reason } => {
            assert!(reason.contains("GHSA-guard"));
        }
        PackageVersionGuardDecision::Allow => panic!("expected rejection, got allow"),
    }
}
