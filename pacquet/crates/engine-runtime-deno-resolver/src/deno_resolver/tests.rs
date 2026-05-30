use std::sync::Arc;

use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{
    LatestInfo, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, WantedDependency,
};

use super::DenoResolver;

struct StubResolver;
impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        _wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async { Ok::<Option<ResolveResult>, ResolveError>(None) })
    }
    fn resolve_latest<'a>(
        &'a self,
        _query: &'a pacquet_resolving_resolver_base::LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok::<Option<LatestInfo>, ResolveError>(None) })
    }
}

fn resolver() -> DenoResolver {
    DenoResolver::new(Arc::new(ThrottledClient::new_for_installs()), Arc::new(StubResolver))
}

/// Non-deno alias is declined.
#[tokio::test]
async fn declines_non_deno_alias() {
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("runtime:1.0.0".to_string()),
        ..WantedDependency::default()
    };
    assert!(resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap().is_none());
}

/// `deno` alias without a `runtime:` prefix is declined.
#[tokio::test]
async fn declines_deno_without_runtime_prefix() {
    let wanted = WantedDependency {
        alias: Some("deno".to_string()),
        bare_specifier: Some("^1.0".to_string()),
        ..WantedDependency::default()
    };
    assert!(resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap().is_none());
}
