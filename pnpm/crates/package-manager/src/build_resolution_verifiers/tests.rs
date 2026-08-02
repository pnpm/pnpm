use std::sync::Arc;

use pacquet_config::Config;
use pacquet_network::ThrottledClient;

use super::{BuildVerifiersError, build_resolution_verifiers};

/// Verifiers are built before the resolver chain that also validates
/// `namedRegistries`, and on the frozen path that chain never runs, so a
/// bad name has to surface here as a diagnostic rather than a panic.
#[test]
fn reserved_named_registry_is_an_error_not_a_panic() {
    let mut config = Config::default();
    config.named_registries.insert("workspace".to_string(), "https://npm.example/".to_string());

    let result =
        build_resolution_verifiers(&config, Arc::new(ThrottledClient::default()), None, None, None);

    assert!(
        matches!(result, Err(BuildVerifiersError::InvalidNamedRegistries { .. })),
        "expected a diagnostic, got {:?}",
        result.map(|verifiers| verifiers.len()),
    );
}
