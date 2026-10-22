use pnpr_osv::OsvIndex;
use std::{collections::HashMap, fs};

#[test]
fn bulk_advisories_matches_vulnerable_versions_and_shapes_payload() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-test-1111.json"),
        r#"{
          "id": "GHSA-test-1111",
          "summary": "Sample XSS in vulnerable package",
          "database_specific": {
            "severity": "HIGH",
            "cwe_ids": ["CWE-79"]
          },
          "affected": [{
            "package": { "ecosystem": "npm", "name": "vulnerable" },
            "ranges": [{
              "type": "SEMVER",
              "events": [
                { "introduced": "0" },
                { "fixed": "2.0.0" }
              ]
            }]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    let mut query = HashMap::new();
    query.insert("vulnerable".to_string(), vec!["1.0.0".to_string()]);
    query.insert("safe-pkg".to_string(), vec!["1.0.0".to_string()]);

    let result = index.bulk_advisories(&query);
    assert_eq!(result.len(), 1);
    let advisories = result.get("vulnerable").expect("advisories for vulnerable");
    assert_eq!(advisories.len(), 1);

    let adv = &advisories[0];
    assert!(adv.id > 0);
    assert_eq!(adv.url, "https://github.com/advisories/GHSA-test-1111");
    assert_eq!(adv.title, "Sample XSS in vulnerable package");
    assert_eq!(adv.severity, "high");
    assert_eq!(adv.vulnerable_versions, "<2.0.0");
    assert_eq!(adv.cwe, vec!["CWE-79"]);
}

#[test]
fn bulk_advisories_omits_non_affected_versions() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-fixed.json"),
        r#"{
          "id": "GHSA-fixed",
          "summary": "Fixed vulnerability",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "vulnerable" },
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

    // Version 1.2.0 is fixed, version 0.9.0 is before introduction
    let mut query = HashMap::new();
    query.insert("vulnerable".to_string(), vec!["0.9.0".to_string(), "1.2.0".to_string()]);

    let result = index.bulk_advisories(&query);
    assert!(result.is_empty(), "non-affected versions must yield no advisories");
}

#[test]
fn bulk_advisories_handles_explicit_range_and_custom_versions() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-custom.json"),
        r#"{
          "id": "GHSA-custom",
          "database_specific": {
            "severity": "CRITICAL"
          },
          "affected": [{
            "package": { "ecosystem": "npm", "name": "custom-pkg" },
            "database_specific": {
              "last_known_affected_version_range": "<= 3.0.0"
            },
            "versions": ["3.0.0-custom"]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    let mut query = HashMap::new();
    query.insert("custom-pkg".to_string(), vec!["3.0.0-custom".to_string()]);

    let result = index.bulk_advisories(&query);
    assert_eq!(result.len(), 1);
    let adv = &result["custom-pkg"][0];
    assert_eq!(adv.severity, "critical");
    assert_eq!(adv.vulnerable_versions, "<= 3.0.0");
}

#[test]
fn bulk_advisories_sorts_out_of_order_events_when_formatting() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-out-of-order.json"),
        r#"{
          "id": "GHSA-out-of-order",
          "summary": "Reversed events advisory",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "reversed" },
            "ranges": [{
              "type": "SEMVER",
              "events": [
                { "fixed": "1.2.0" },
                { "introduced": "1.0.0" }
              ]
            }]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    let mut query = HashMap::new();
    query.insert("reversed".to_string(), vec!["1.1.0".to_string()]);

    let result = index.bulk_advisories(&query);
    assert_eq!(result.len(), 1);
    let adv = &result["reversed"][0];
    assert_eq!(adv.vulnerable_versions, ">=1.0.0 <1.2.0");
}
