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

#[test]
fn enabled_database_without_npm_advisories_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Only a non-npm advisory, so the npm index ends up empty.
    fs::write(
        dir.path().join("GHSA-pypi.json"),
        r#"{
          "id": "GHSA-pypi",
          "affected": [{
            "package": { "ecosystem": "PyPI", "name": "x" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write record");

    let listen = "127.0.0.1:4873".parse().expect("listen addr");
    let mut config = crate::Config::proxy(listen, dir.path().to_path_buf());
    config.osv.enabled = true;
    config.osv.path = Some(dir.path().to_path_buf());

    let err = super::load_osv_index(&config).expect_err("an empty npm index must be rejected");
    assert!(format!("{err}").contains("no npm advisories"));
}
