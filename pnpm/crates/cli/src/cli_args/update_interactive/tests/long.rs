use super::super::{
    InteractiveUpdateProject, UpdateDetails, choices::update_choices, collect_choices,
};
use crate::cli_args::outdated::TargetVersion;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn long_fetches_full_metadata_and_displays_homepage() {
    check_details(UpdateDetails::Long, json!({
        "homepage": "https://example.com/home", "repository": { "url": "https://example.com/repo" }
    }), Some("https://example.com/home")).await;
}

#[tokio::test]
async fn compact_uses_abbreviated_metadata_and_hides_urls() {
    check_details(UpdateDetails::Compact, json!({
        "homepage": "https://example.com/home", "repository": { "url": "https://example.com/repo" }
    }), None).await;
}

#[tokio::test]
async fn long_falls_back_to_repository_object() {
    check_details(
        UpdateDetails::Long,
        json!({
            "repository": { "url": "https://example.com/repo" }
        }),
        Some("https://example.com/repo"),
    )
    .await;
}

#[tokio::test]
async fn long_falls_back_to_repository_string() {
    check_details(
        UpdateDetails::Long,
        json!({
            "repository": "https://example.com/repo"
        }),
        Some("https://example.com/repo"),
    )
    .await;
}

#[tokio::test]
async fn long_allows_missing_urls() {
    check_details(UpdateDetails::Long, json!({}), None).await;
}

async fn check_details(
    details: UpdateDetails,
    url_fields: serde_json::Value,
    expected_url: Option<&str>,
) {
    let accept = match details {
        UpdateDetails::Long => "application/json; q=1.0, */*",
        UpdateDetails::Compact => {
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*"
        }
    };
    let temp = tempfile::tempdir().expect("create project");
    let manifest = super::manifest_with_dependency(temp.path(), "packages/a", "foo");
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters:\n  packages/a:\n    dependencies:\n      foo:\n        specifier: ^1.0.0\n        version: 1.0.0\n",
    ).expect("parse lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let mut target = json!({ "name": "foo", "version": "1.0.1", "dist": {
        "tarball": format!("{registry}foo/-/foo-1.0.1.tgz"), "integrity": super::TEST_INTEGRITY
    } });
    target
        .as_object_mut()
        .unwrap()
        .extend(url_fields.as_object().unwrap().clone());
    let metadata = server.mock("GET", "/foo")
        .match_header("accept", accept)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({
            "name": "foo",
            "dist-tags": { "latest": "1.0.1" },
            "versions": {
                "1.0.0": { "name": "foo", "version": "1.0.0", "dist": { "tarball": format!("{registry}foo/-/foo-1.0.0.tgz"), "integrity": super::TEST_INTEGRITY } },
                "1.0.1": target
            }
        }).to_string())
        .expect(1)
        .create_async().await;
    let mut config = Config::new();
    config.registry = registry;
    config.cache_dir = temp.path().join("cache");
    config.minimum_release_age = Some(0);
    let projects =
        [InteractiveUpdateProject { manifest: &manifest, importer_id: "packages/a".to_string() }];
    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &Arc::new(ThrottledClient::default()),
        TargetVersion::Latest,
        &[DependencyGroup::Prod],
        details,
    )
    .await
    .expect("collect choices");
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].target.to_string(), "1.0.1");
    for workspaces_enabled in [false, true] {
        let groups =
            update_choices(&choices.iter().collect::<Vec<_>>(), workspaces_enabled, details);
        assert_eq!(groups[0].rows[0].label.contains("URL"), details.shows_urls());
        assert_eq!(groups[0].rows[0].label.contains("Workspace"), workspaces_enabled);
        let url = groups[0].rows[1].label
            .split_whitespace()
            .find(|cell| cell.starts_with("https://"));
        assert_eq!(url, expected_url);
        assert_eq!(groups[0].rows[1].value.as_deref(), Some("foo"));
    }
    metadata.assert_async().await;
}
