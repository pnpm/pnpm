use super::{DistTagContext, registry::registry_for_package};
use pnpm_config::Config;
use std::collections::HashMap;

#[test]
fn registry_for_package_uses_scoped_registry_by_default() {
    let config = Config::default();
    let registries = HashMap::from([
        ("default".to_string(), "https://registry.example/npm/".to_string()),
        ("@scope".to_string(), "https://scoped.example/npm/".to_string()),
    ]);
    let context = DistTagContext {
        config: &config,
        http_client: pnpm_network::ThrottledClient::default(),
        retry_opts: pnpm_network::RetryOpts::default(),
        registries,
        registry_override: None,
        otp: None,
    };
    let registry = registry_for_package(&context, "@scope/my-pkg");
    assert_eq!(registry, "https://scoped.example/npm/");
}

#[test]
fn registry_for_package_honors_registry_override_for_scoped_package() {
    let config = Config::default();
    let registries = HashMap::from([
        ("default".to_string(), "https://registry.example/npm/".to_string()),
        ("@scope".to_string(), "https://scoped.example/npm/".to_string()),
    ]);
    let context = DistTagContext {
        config: &config,
        http_client: pnpm_network::ThrottledClient::default(),
        retry_opts: pnpm_network::RetryOpts::default(),
        registries,
        registry_override: Some("https://override.example/npm/"),
        otp: None,
    };
    let registry = registry_for_package(&context, "@scope/my-pkg");
    assert_eq!(registry, "https://override.example/npm/");
}
