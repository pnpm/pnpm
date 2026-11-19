use std::{collections::HashMap, fs};

use node_semver::{Range, Version};
use pnpr_osv::OsvIndex;

#[test]
fn bulk_advisories_matches_vulnerable_versions_and_shapes_payload() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-2222-3333-4444.json"),
        r#"{
          "id": "GHSA-2222-3333-4444",
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
    assert_eq!(adv.url, "https://github.com/advisories/GHSA-2222-3333-4444");
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
    dbg!(&result);
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
    assert_eq!(adv.vulnerable_versions, "<= 3.0.0 || 3.0.0-custom");

    let parsed_range: Range = adv.vulnerable_versions.parse().expect("parse semver range");
    let custom_ver = Version::parse("3.0.0-custom").expect("parse custom version");
    dbg!(&parsed_range, &custom_ver);
    assert!(custom_ver.satisfies(&parsed_range));
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

#[test]
fn bulk_advisories_unions_ranges_with_uncovered_versions_and_dedupes_covered() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-0000-0000-0001.json"),
        r#"{
          "id": "GHSA-0000-0000-0001",
          "summary": "Backported regression outside primary range",
          "database_specific": { "severity": "CRITICAL" },
          "affected": [{
            "package": { "ecosystem": "npm", "name": "@acme/vuln" },
            "ranges": [{
              "type": "SEMVER",
              "events": [{ "introduced": "1.0.0" }, { "fixed": "1.0.5" }]
            }],
            "versions": ["1.x", "1.0.2", "2.0.1"]
          }]
        }"#,
    )
    .expect("write mixed record");
    fs::write(
        dir.path().join("GHSA-0000-0000-0002.json"),
        r#"{
          "id": "GHSA-0000-0000-0002",
          "summary": "Enumerated versions already inside range",
          "database_specific": { "severity": "HIGH" },
          "affected": [{
            "package": { "ecosystem": "npm", "name": "@acme/covered" },
            "ranges": [{
              "type": "SEMVER",
              "events": [{ "introduced": "1.0.0" }, { "fixed": "1.0.5" }]
            }],
            "versions": ["1.0.0", "1.0.1", "1.0.4"]
          }]
        }"#,
    )
    .expect("write covered record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    let mut query = HashMap::new();
    query.insert("@acme/vuln".to_string(), vec!["2.0.1".to_string()]);
    query.insert("@acme/covered".to_string(), vec!["1.0.2".to_string()]);

    let result = index.bulk_advisories(&query);
    let mixed = &result["@acme/vuln"][0];
    assert_eq!(mixed.vulnerable_versions, ">=1.0.0 <1.0.5 || 2.0.1");

    let parsed_mixed: Range = mixed.vulnerable_versions.parse().expect("parse union range");
    let target_ver = Version::parse("2.0.1").expect("parse 2.0.1");
    dbg!(&parsed_mixed, &target_ver);
    assert!(target_ver.satisfies(&parsed_mixed));

    let covered = &result["@acme/covered"][0];
    assert_eq!(covered.vulnerable_versions, ">=1.0.0 <1.0.5");
}

#[test]
fn bulk_advisories_skips_git_ranges_and_handles_limit_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-git-only.json"),
        r#"{
          "id": "GHSA-git-only",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "git-pkg" },
            "ranges": [{
              "type": "GIT",
              "events": [
                { "introduced": "0" },
                { "fixed": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4" }
              ]
            }],
            "versions": ["1.2.3"]
          }]
        }"#,
    )
    .expect("write git record");
    fs::write(
        dir.path().join("GHSA-limit-open.json"),
        r#"{
          "id": "GHSA-limit-open",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "limit-pkg" },
            "ranges": [{
              "type": "SEMVER",
              "events": [
                { "introduced": "1.2.0" },
                { "limit": "1.3.0" }
              ]
            }]
          }]
        }"#,
    )
    .expect("write limit record");
    fs::write(
        dir.path().join("GHSA-fixed-then-limit.json"),
        r#"{
          "id": "GHSA-fixed-then-limit",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "fixed-limit-pkg" },
            "ranges": [{
              "type": "SEMVER",
              "events": [
                { "introduced": "1.0.0" },
                { "fixed": "2.0.0" },
                { "limit": "3.0.0" }
              ]
            }]
          }]
        }"#,
    )
    .expect("write fixed-then-limit record");
    fs::write(
        dir.path().join("GHSA-multi-limit.json"),
        r#"{
          "id": "GHSA-multi-limit",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "multi-limit-pkg" },
            "ranges": [{
              "type": "SEMVER",
              "events": [
                { "introduced": "1.0.0" },
                { "limit": "2.0.0" },
                { "limit": "3.0.0" }
              ]
            }]
          }]
        }"#,
    )
    .expect("write multi-limit record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");
    assert!(index.is_vulnerable("multi-limit-pkg", "2.5.0"));
    assert!(!index.is_vulnerable("multi-limit-pkg", "3.0.0"));

    let mut query = HashMap::new();
    query.insert("git-pkg".to_string(), vec!["1.2.3".to_string()]);
    query.insert("limit-pkg".to_string(), vec!["1.2.5".to_string()]);
    query.insert("fixed-limit-pkg".to_string(), vec!["1.5.0".to_string()]);
    query.insert("multi-limit-pkg".to_string(), vec!["2.5.0".to_string()]);

    let result = index.bulk_advisories(&query);
    assert_eq!(result["git-pkg"][0].vulnerable_versions, "1.2.3");
    assert_eq!(result["limit-pkg"][0].vulnerable_versions, ">=1.2.0 <1.3.0");
    assert_eq!(result["fixed-limit-pkg"][0].vulnerable_versions, ">=1.0.0 <2.0.0");
    assert_eq!(result["multi-limit-pkg"][0].vulnerable_versions, ">=1.0.0 <3.0.0");
}

#[test]
fn bulk_advisories_extracts_cvss_and_ecosystem_severities() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("MAL-2026-0001.json"),
        r#"{
          "id": "MAL-2026-0001",
          "summary": "CVSS 10.0 advisory",
          "severity": [{
            "type": "CVSS_V3",
            "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H"
          }],
          "affected": [{
            "package": { "ecosystem": "npm", "name": "cvss-crit" },
            "ranges": [{ "type": "SEMVER", "events": [{ "introduced": "2.0.0" }] }]
          }]
        }"#,
    )
    .expect("write crit record");
    fs::write(
        dir.path().join("OSV-2026-v4.json"),
        r#"{
          "id": "OSV-2026-v4",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "cvss-v4-high" },
            "severity": [{
              "type": "CVSS_V4",
              "score": "CVSS:4.0/AV:N/AC:L/AT:N/PR:L/UI:N/VC:H/VI:L/VA:N/SC:N/SI:N/SA:N"
            }],
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write v4 record");
    fs::write(
        dir.path().join("OSV-2026-eco.json"),
        r#"{
          "id": "OSV-2026-eco",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "eco-sev" },
            "ecosystem_specific": { "severity": "HIGH" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write eco record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    let mut query = HashMap::new();
    query.insert("cvss-crit".to_string(), vec!["2.0.1".to_string()]);
    query.insert("cvss-v4-high".to_string(), vec!["1.0.0".to_string()]);
    query.insert("eco-sev".to_string(), vec!["1.0.0".to_string()]);

    let result = index.bulk_advisories(&query);
    assert_eq!(result["cvss-crit"][0].severity, "critical");
    assert_eq!(result["cvss-v4-high"][0].severity, "high");
    assert_eq!(result["eco-sev"][0].severity, "high");
}

#[test]
fn bulk_advisories_resolves_non_ghsa_and_aliased_advisory_urls() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("MAL-2026-0001.json"),
        r#"{
          "id": "MAL-2026-0001",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "mal-plain" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write plain mal record");
    fs::write(
        dir.path().join("MAL-2026-0002.json"),
        r#"{
          "id": "MAL-2026-0002",
          "aliases": ["CVE-2026-0002", "GHSA-2345-6789-cfgh"],
          "affected": [{
            "package": { "ecosystem": "npm", "name": "mal-aliased" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write aliased mal record");
    fs::write(
        dir.path().join("MAL-2026-0004.json"),
        r#"{
          "id": "MAL-2026-0004",
          "aliases": ["GHSA-aaaa-bbbb-cccc"],
          "references": [{
            "type": "ADVISORY",
            "url": "https://security.example.com/advisories/MAL-2026-0004"
          }],
          "affected": [{
            "package": { "ecosystem": "npm", "name": "mal-invalid-alias" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write invalid-alias mal record");
    fs::write(
        dir.path().join("OSV-2026-0003.json"),
        r#"{
          "id": "OSV-2026-0003",
          "references": [
            { "type": "ADVISORY", "url": "https://" },
            { "type": "ADVISORY", "url": "https://security.example.com/advisories/OSV-2026-0003" }
          ],
          "affected": [{
            "package": { "ecosystem": "npm", "name": "osv-ref" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write ref record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    let mut query = HashMap::new();
    query.insert("mal-plain".to_string(), vec!["1.0.0".to_string()]);
    query.insert("mal-aliased".to_string(), vec!["1.0.0".to_string()]);
    query.insert("mal-invalid-alias".to_string(), vec!["1.0.0".to_string()]);
    query.insert("osv-ref".to_string(), vec!["1.0.0".to_string()]);

    let result = index.bulk_advisories(&query);
    assert_eq!(result["mal-plain"][0].url, "https://osv.dev/vulnerability/MAL-2026-0001");
    assert_eq!(result["mal-aliased"][0].url, "https://github.com/advisories/GHSA-2345-6789-cfgh");
    assert_eq!(
        result["mal-invalid-alias"][0].url,
        "https://security.example.com/advisories/MAL-2026-0004",
    );
    assert_eq!(result["osv-ref"][0].url, "https://security.example.com/advisories/OSV-2026-0003");
}
