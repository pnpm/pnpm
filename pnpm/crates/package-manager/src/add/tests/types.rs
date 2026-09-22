use super::{
    package_body,
    test_add,
};
use crate::add::manifest::prepare_single_add;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::SilentReporter;

#[tokio::test]
async fn type_discovery_shares_metadata_without_fetching_unused_formats() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = mockito::Server::new_async().await;
    let registry_url = format!("{}/", registry.url());
    let mut requests = Vec::new();
    for name in ["foo", "bar", "@types/foo", "@types/bar"] {
        for (full, accept) in [
            (false, "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*"),
            (true, "application/json; q=1.0, */*"),
        ] {
            let expected = usize::from(full != name.starts_with("@types/"));
            requests.push(
                registry
                    .mock(
                        "GET",
                        mockito::Matcher::Regex(format!("(?i)^/{}$", name.replace('/', "%2f"))),
                    )
                    .match_header("accept", accept)
                    .with_header("content-type", "application/json")
                    .with_body(package_body(name, &registry_url))
                    .expect(expected)
                    .create_async()
                    .await,
            );
        }
    }
    let mut config = Config::new();
    config.registry = registry_url;
    config.minimum_release_age = None;
    config.cache_dir = dir.path().join("cache");
    let config = config.leak();
    let http_client = ThrottledClient::default();
    let names = ["foo@^1".to_string(), "bar@^1".to_string()];
    let (mut options, resources) = test_add(config, &http_client, &names, None);
    options.save_types = true;
    let mut manifest = PackageManifest::create_if_needed(dir.path().join("package.json")).unwrap();
    prepare_single_add::<SilentReporter>(options, &resources, &mut manifest)
        .await
        .unwrap();
    for request in requests {
        request.assert_async().await;
    }
}

#[test]
fn runtime_and_companion_policies_share_the_add_release_age_cutoff() {
    let mut config = Config::new();
    config.minimum_release_age = Some(60);
    let config = config.leak();
    let http_client = ThrottledClient::default();
    let names = [];
    let (mut add, owned) = test_add(config, &http_client, &names, None);
    add.save_types = true;
    let started_at = chrono::DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z").unwrap().to_utc();
    let resolution = crate::add::AddResolution { started_at, ..crate::add::AddResolution::new() };
    let inputs = crate::add::AddResolveInputs {
        add,
        owned: &owned,
        git_source_cache: &std::sync::Arc::default(),
        resolution: &resolution,
        preferred_versions: &std::sync::OnceLock::new(),
        catalogs: &pnpm_catalogs_types::Catalogs::new(),
        prefix: "",
        workspace_packages: None,
    };
    let runtime = crate::add::registry::add_pick_policy(&inputs, "foo").unwrap();
    let companion = crate::add::registry::add_pick_policy(&inputs, "@types/foo").unwrap();
    let expected = Some(started_at - chrono::Duration::minutes(60));
    assert_eq!(runtime.published_by, expected);
    assert_eq!(companion.published_by, expected);
}
