use super::UnavailableToolResolver;
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};

#[tokio::test(flavor = "current_thread")]
async fn rejects_tool_runtime_requests_before_local_path_resolution() {
    let resolver = UnavailableToolResolver;
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("runtime:rc/22.0.0".to_string()),
        ..WantedDependency::default()
    };
    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("runtime provisioning is disabled");

    assert_eq!(
        error.to_string(),
        "Runtime provisioning is not enabled in this build for node@runtime:rc/22.0.0",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn leaves_registry_requests_to_the_named_registry_resolver() {
    let resolver = UnavailableToolResolver;
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("^22.0.0".to_string()),
        ..WantedDependency::default()
    };

    assert!(
        resolver
            .resolve(&wanted, &ResolveOptions::default())
            .await
            .expect("registry requests are not tool requests")
            .is_none(),
    );
}
