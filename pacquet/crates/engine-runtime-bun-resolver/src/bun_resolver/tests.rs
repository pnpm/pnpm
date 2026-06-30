use std::sync::{Arc, Mutex};

use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{
    LatestInfo, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, WantedDependency,
};

use super::BunResolver;

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

#[derive(Default)]
struct CapturingResolver {
    seen: Mutex<Vec<Option<String>>>,
}

impl Resolver for CapturingResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        self.seen.lock().unwrap().push(wanted_dependency.bare_specifier.clone());
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

fn resolver() -> BunResolver {
    BunResolver::new(Arc::new(ThrottledClient::new_for_installs()), Arc::new(StubResolver))
}

#[tokio::test]
async fn declines_non_bun_alias() {
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("runtime:1.0.0".to_string()),
        ..WantedDependency::default()
    };
    assert!(resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap().is_none());
}

#[tokio::test]
async fn declines_bun_without_runtime_prefix() {
    let wanted = WantedDependency {
        alias: Some("bun".to_string()),
        bare_specifier: Some("^1.0".to_string()),
        ..WantedDependency::default()
    };
    assert!(resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap().is_none());
}

#[tokio::test]
async fn empty_runtime_spec_delegates_latest_to_npm_resolver() {
    let npm_resolver = Arc::new(CapturingResolver::default());
    let resolver = BunResolver::new(
        Arc::new(ThrottledClient::new_for_installs()),
        Arc::<CapturingResolver>::clone(&npm_resolver),
    );
    let wanted = WantedDependency {
        alias: Some("bun".to_string()),
        bare_specifier: Some("runtime:".to_string()),
        ..WantedDependency::default()
    };

    resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();

    assert_eq!(npm_resolver.seen.lock().unwrap().clone(), vec![Some("latest".to_string())]);
}

#[tokio::test]
async fn whitespace_runtime_spec_delegates_latest_to_npm_resolver() {
    let npm_resolver = Arc::new(CapturingResolver::default());
    let resolver = BunResolver::new(
        Arc::new(ThrottledClient::new_for_installs()),
        Arc::<CapturingResolver>::clone(&npm_resolver),
    );
    let wanted = WantedDependency {
        alias: Some("bun".to_string()),
        bare_specifier: Some("runtime:  ".to_string()),
        ..WantedDependency::default()
    };

    resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();

    assert_eq!(npm_resolver.seen.lock().unwrap().clone(), vec![Some("latest".to_string())]);
}
