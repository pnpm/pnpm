use std::collections::BTreeMap;

use pacquet_config::Config as PacquetConfig;

use super::{
    protocol::{ResolveRequest, ResolveRequestProject},
    resolution_cache_key,
};

fn config() -> PacquetConfig {
    let mut config = PacquetConfig::new();
    config.registry = "https://registry.example.test/".to_string();
    config
}

fn deps(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(name, spec)| ((*name).to_string(), (*spec).to_string())).collect()
}

#[test]
fn resolution_cache_key_normalizes_single_project_requests() {
    let top_level = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    let projects = ResolveRequest {
        projects: Some(vec![ResolveRequestProject {
            dir: ".".to_string(),
            dependencies: deps(&[("foo", "^1.0.0")]),
            ..ResolveRequestProject::default()
        }]),
        ..ResolveRequest::default()
    };

    assert_eq!(
        resolution_cache_key(&config(), &top_level),
        resolution_cache_key(&config(), &projects),
    );
}

#[test]
fn resolution_cache_key_changes_with_dependencies_and_policy() {
    let base = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    let different_dep = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^2.0.0")])),
        ..ResolveRequest::default()
    };
    let different_policy = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        minimum_release_age: Some(60),
        ..ResolveRequest::default()
    };

    let config = config();
    let base_key = resolution_cache_key(&config, &base);

    assert_ne!(base_key, resolution_cache_key(&config, &different_dep));
    assert_ne!(base_key, resolution_cache_key(&config, &different_policy));
}
