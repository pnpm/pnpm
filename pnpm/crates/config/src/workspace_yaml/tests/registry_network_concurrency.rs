//! The `networkConcurrency` a `registries` entry sets.

use super::{Config, Path, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, fs};
use std::num::NonZeroUsize;

fn load(yaml: &str) -> Result<Config, String> {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), yaml).unwrap();
    let settings = WorkspaceSettings::load_at(dir.path())
        .map_err(|error| error.to_string())?
        .expect("the workspace manifest was just written");
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));
    Ok(config)
}

fn limit(value: usize) -> Option<NonZeroUsize> {
    NonZeroUsize::new(value)
}

#[test]
fn an_entry_caps_its_registry_in_the_install_client_settings() {
    let config = load(
        "registries:\n  https://npm.corp.example/npm:\n    scopes: ['@acme']\n    networkConcurrency: 4\n",
    )
    .unwrap();
    let settings = config.network_settings();
    assert_eq!(
        settings.network_concurrency_by_registry.get("https://npm.corp.example/npm/").copied(),
        limit(4),
    );
    assert_eq!(
        config.registries_by_scope.get("@acme").map(String::as_str),
        Some("https://npm.corp.example/npm/"),
    );
}

#[test]
fn an_entry_may_set_only_a_concurrency() {
    let config =
        load("registries:\n  https://registry.npmjs.org/:\n    networkConcurrency: 8\n").unwrap();
    assert_eq!(
        config.network_concurrency_by_registry.get("https://registry.npmjs.org/").copied(),
        limit(8),
    );
    assert!(config.registries_by_scope.is_empty());
}

#[test]
fn a_zero_concurrency_is_rejected() {
    let error =
        load("registries:\n  https://npm.corp.example/:\n    networkConcurrency: 0\n").unwrap_err();
    assert!(error.contains("networkConcurrency") || error.contains("nonzero"), "{error}");
}

#[test]
fn the_resolved_view_shows_the_concurrency_and_a_pnpr_request_omits_it() {
    let config = load(
        "registries:\n  https://npm.corp.example/:\n    scopes: ['@acme']\n    networkConcurrency: 4\n",
    )
    .unwrap();
    let resolved = config.resolved_registry_declarations();
    assert_eq!(resolved["https://npm.corp.example/"].network_concurrency, limit(4));
    let for_pnpr = config.registry_declarations();
    assert_eq!(for_pnpr["https://npm.corp.example/"].network_concurrency, None);
}

#[test]
fn the_global_config_may_set_a_registry_concurrency() {
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(
        "registries:\n  https://npm.corp.example/:\n    serverType: artifactory\n    networkConcurrency: 2\n",
    )
    .unwrap();
    settings.clear_workspace_only_fields();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.network_concurrency_by_registry.get("https://npm.corp.example/").copied(),
        limit(2),
    );
    assert!(config.registry_options_by_url.is_empty(), "serverType stays workspace-only");
}

#[test]
fn the_resolved_view_keeps_a_prefixed_registry_in_one_entry() {
    let config = load(
        "registries:\n  https://npm.corp.example/npm:\n    prefix: work\n    networkConcurrency: 4\n",
    )
    .unwrap();
    let resolved = config.resolved_registry_declarations();
    let corp: Vec<_> = resolved
        .iter()
        .filter(|(registry, _)| registry.contains("npm.corp.example"))
        .collect();
    assert_eq!(corp.len(), 1, "{resolved:?}");
    assert_eq!(corp[0].1.prefix.as_deref(), Some("work"));
    assert_eq!(corp[0].1.network_concurrency, limit(4));
}

#[test]
fn the_resolved_view_caps_every_spelling_of_one_registry() {
    let config = load(
        "registries:\n  https://npm.corp.example/npm:\n    prefix: work\n    scopes: ['@acme']\n    networkConcurrency: 4\n",
    )
    .unwrap();
    let resolved = config.resolved_registry_declarations();
    let corp: Vec<_> = resolved
        .iter()
        .filter(|(registry, _)| registry.contains("npm.corp.example"))
        .collect();
    assert!(!corp.is_empty(), "{resolved:?}");
    for (registry, declaration) in corp {
        assert_eq!(declaration.network_concurrency, limit(4), "{registry}");
    }
}

#[test]
fn two_spellings_of_one_registry_keep_the_smallest_cap() {
    let config = load(
        "registries:\n  https://npm.corp.example/npm:\n    networkConcurrency: 2\n  https://npm.corp.example/npm/:\n    networkConcurrency: 6\n",
    )
    .unwrap();
    assert_eq!(
        config.network_concurrency_by_registry.get("https://npm.corp.example/npm/").copied(),
        limit(2),
    );
}
