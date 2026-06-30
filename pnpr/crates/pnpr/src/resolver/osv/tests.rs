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

    assert!(!index.is_vulnerable("acme", "0.9.0"));
    assert!(index.is_vulnerable("acme", "1.1.0"));
    assert!(!index.is_vulnerable("acme", "1.2.0"));
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

#[test]
fn package_name_lookup_is_case_insensitive() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("GHSA-case.json"),
        r#"{
          "id": "GHSA-case",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "JSONStream" },
            "versions": ["1.0.0"]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    assert_eq!(index.vulnerability_ids("jsonstream", "1.0.0"), vec!["GHSA-case"]);
    assert_eq!(index.vulnerability_ids("JSONStream", "1.0.0"), vec!["GHSA-case"]);
}

#[test]
fn duplicate_affected_blocks_yield_one_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    // One record listing the same package twice — the id must appear once.
    fs::write(
        dir.path().join("GHSA-dup.json"),
        r#"{
          "id": "GHSA-dup",
          "affected": [
            { "package": { "ecosystem": "npm", "name": "dup" }, "versions": ["1.0.0"] },
            { "package": { "ecosystem": "npm", "name": "dup" },
              "ranges": [{ "type": "SEMVER", "events": [{ "introduced": "1.0.0" }] }] }
          ]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    assert_eq!(index.vulnerability_ids("dup", "1.0.0"), vec!["GHSA-dup"]);
}

#[test]
fn introduced_zero_covers_prerelease_versions() {
    let dir = tempfile::tempdir().expect("tempdir");
    // `introduced: "0"` means "all versions"; a prerelease below 0.0.0 must
    // still be covered.
    fs::write(
        dir.path().join("GHSA-zero.json"),
        r#"{
          "id": "GHSA-zero",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "zero" },
            "ranges": [{ "type": "SEMVER", "events": [{ "introduced": "0" }, { "fixed": "2.0.0" }] }]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    assert_eq!(index.vulnerability_ids("zero", "0.0.0-alpha.1"), vec!["GHSA-zero"]);
    assert_eq!(index.vulnerability_ids("zero", "1.0.0"), vec!["GHSA-zero"]);
    assert_eq!(index.vulnerability_ids("zero", "2.0.0"), Vec::<String>::new());
}

#[test]
fn out_of_order_range_events_are_normalized() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Events are deliberately reversed (fixed before introduced). Without
    // sorting, a version above the fix would be walked as still-affected.
    fs::write(
        dir.path().join("GHSA-order.json"),
        r#"{
          "id": "GHSA-order",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "ord" },
            "ranges": [{
              "type": "SEMVER",
              "events": [ { "fixed": "1.2.0" }, { "introduced": "1.0.0" } ]
            }]
          }]
        }"#,
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    assert_eq!(index.vulnerability_ids("ord", "0.9.0"), Vec::<String>::new());
    assert_eq!(index.vulnerability_ids("ord", "1.1.0"), vec!["GHSA-order"]);
    // Above the fix: must be safe — this is the case that fails without sorting.
    assert_eq!(index.vulnerability_ids("ord", "1.3.0"), Vec::<String>::new());
}

#[test]
fn withdrawn_handling_respects_null_vs_timestamp() {
    let dir = tempfile::tempdir().expect("tempdir");
    // `withdrawn: null` is not a withdrawal — the advisory stays active.
    fs::write(
        dir.path().join("GHSA-active.json"),
        r#"{ "id": "GHSA-active", "withdrawn": null,
            "affected": [{ "package": { "ecosystem": "npm", "name": "pkg" }, "versions": ["1.0.0"] }] }"#,
    )
    .expect("write active record");
    // A real withdrawal timestamp drops the advisory.
    fs::write(
        dir.path().join("GHSA-gone.json"),
        r#"{ "id": "GHSA-gone", "withdrawn": "2024-01-01T00:00:00Z",
            "affected": [{ "package": { "ecosystem": "npm", "name": "pkg" }, "versions": ["1.0.0"] }] }"#,
    )
    .expect("write withdrawn record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    assert_eq!(index.vulnerability_ids("pkg", "1.0.0"), vec!["GHSA-active"]);
}

#[test]
fn loads_zip_archive_and_fingerprints_it() {
    use std::io::Write;

    let dir = tempfile::tempdir().expect("tempdir");
    let zip_path = dir.path().join("all.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&zip_path).expect("create zip"));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("GHSA-zip.json", options).expect("start file");
    writer
        .write_all(
            br#"{"id":"GHSA-zip","affected":[{"package":{"ecosystem":"npm","name":"zipped"},"versions":["1.0.0"]}]}"#,
        )
        .expect("write entry");
    writer.finish().expect("finish zip");

    let index = OsvIndex::load_from_path(&zip_path).expect("load zip index");
    assert_eq!(index.vulnerability_ids("zipped", "1.0.0"), vec!["GHSA-zip"]);
}

#[cfg(unix)]
#[test]
fn non_regular_file_path_is_rejected() {
    // A socket is neither a directory nor a regular file; loading it as a
    // zip would risk blocking on the read, so it must fail fast instead.
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("osv.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket_path).expect("bind socket");

    let err = OsvIndex::load_from_path(&socket_path).expect_err("a socket path must be rejected");
    assert!(format!("{err}").contains("neither a directory nor a regular file"), "{err}");
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

#[test]
fn advisory_ids_are_capped_in_messages() {
    let few: Vec<String> = (0..3).map(|i| format!("GHSA-{i}")).collect();
    assert_eq!(super::format_advisory_ids(&few), "GHSA-0, GHSA-1, GHSA-2");

    let many: Vec<String> = (0..25).map(|i| format!("GHSA-{i}")).collect();
    let formatted = super::format_advisory_ids(&many);
    assert!(formatted.ends_with("and 5 more"), "{formatted}");
    assert_eq!(formatted.matches("GHSA-").count(), 20);
}

#[test]
fn oversized_advisory_id_is_truncated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let big_id = format!("GHSA-{}", "x".repeat(5000));
    fs::write(
        dir.path().join("GHSA-big.json"),
        format!(
            r#"{{"id":"{big_id}","affected":[{{"package":{{"ecosystem":"npm","name":"big"}},"versions":["1.0.0"]}}]}}"#,
        ),
    )
    .expect("write record");

    let index = OsvIndex::load_from_path(dir.path()).expect("load index");

    let ids = index.vulnerability_ids("big", "1.0.0");
    assert_eq!(ids.len(), 1);
    assert!(ids[0].len() < 300, "id not truncated: {} bytes", ids[0].len());
    assert!(ids[0].ends_with('…'), "truncated id should end with ellipsis");
}
