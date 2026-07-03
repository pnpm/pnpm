use super::{publish_eligible, write_publish_summary};
use pacquet_publish::{PackedPkgInfo, create_publish_summary};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn named_versioned_public_package_is_eligible() {
    let manifest = json!({ "name": "pkg", "version": "1.2.3" });
    assert_eq!(publish_eligible(&manifest), Some(("pkg", "1.2.3")));
}

#[test]
fn private_package_is_skipped() {
    let manifest = json!({ "name": "pkg", "version": "1.2.3", "private": true });
    assert_eq!(publish_eligible(&manifest), None);
}

#[test]
fn explicit_non_private_is_eligible() {
    let manifest = json!({ "name": "pkg", "version": "1.2.3", "private": false });
    assert_eq!(publish_eligible(&manifest), Some(("pkg", "1.2.3")));
}

#[test]
fn missing_name_or_version_is_skipped() {
    assert_eq!(publish_eligible(&json!({ "version": "1.2.3" })), None);
    assert_eq!(publish_eligible(&json!({ "name": "pkg" })), None);
    assert_eq!(publish_eligible(&json!({})), None);
}

fn summary_for(name: &str, version: &str) -> pacquet_publish::PublishSummary {
    let manifest = json!({ "name": name, "version": version });
    create_publish_summary(
        &PackedPkgInfo {
            published_manifest: &manifest,
            tarball_path: &format!("{name}-{version}.tgz"),
            contents: &[],
            unpacked_size: 0,
        },
        b"tarball",
    )
}

#[test]
fn report_summary_wraps_the_packages_under_published_packages() {
    let dir = tempfile::tempdir().expect("a workspace dir");
    write_publish_summary(dir.path(), &[summary_for("pkg", "1.0.0")]).expect("write the summary");

    let written = std::fs::read_to_string(dir.path().join("pnpm-publish-summary.json"))
        .expect("the summary file is written");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("valid JSON");
    assert_eq!(parsed["publishedPackages"][0]["name"], "pkg");
    assert_eq!(parsed["publishedPackages"][0]["id"], "pkg@1.0.0");
}

#[test]
fn report_summary_writes_an_empty_list_when_nothing_was_published() {
    let dir = tempfile::tempdir().expect("a workspace dir");
    write_publish_summary(dir.path(), &[]).expect("write the summary");

    let written = std::fs::read_to_string(dir.path().join("pnpm-publish-summary.json"))
        .expect("the summary file is written");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("valid JSON");
    assert_eq!(parsed, json!({ "publishedPackages": [] }));
}
