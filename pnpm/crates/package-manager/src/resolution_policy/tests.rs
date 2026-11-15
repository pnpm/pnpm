use super::{PickPolicy, create_configured_registry_resolver};
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_resolving_resolver_base::{ResolveOptions, WantedDependency};
use std::sync::Arc;

#[tokio::test]
async fn runtime_requests_report_unavailable_provisioning() {
    let mut config = Config::new();
    // Nothing listens here, so a request that reaches the network fails
    // with a connection error, not the provisioning error.
    config.registry = "http://127.0.0.1:9/".to_string();
    let policy = PickPolicy::from_config(&config).expect("derive pick policy");
    let resolver =
        create_configured_registry_resolver(&config, Arc::new(ThrottledClient::default()), &policy)
            .expect("build resolver");
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("runtime:22".to_string()),
        ..WantedDependency::default()
    };

    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("runtime provisioning is disabled");

    assert_eq!(
        error.to_string(),
        "Runtime provisioning is not enabled in this build for node@runtime:22",
    );
}
