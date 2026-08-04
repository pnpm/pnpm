use pacquet_lockfile::{LockfileResolution, PkgNameVer, RegistryResolution};
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, WantedDependency,
};
use ssri::Integrity;

use crate::{DefaultResolver, SpecNotSupportedByAnyResolverError};

fn fake_resolution() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
            .parse::<Integrity>()
            .expect("parse fake integrity"),
    })
}

fn fake_name_ver() -> PkgNameVer {
    "lodash@4.17.21".parse().expect("parse fake PkgNameVer")
}

/// Resolver that claims any wanted dep whose `bare_specifier` starts
/// with the configured prefix, returning a stub result tagged with the
/// configured `resolved_via`. Returns `Ok(None)` otherwise.
struct PrefixResolver {
    prefix: &'static str,
    tag: &'static str,
}

impl Resolver for PrefixResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let bare = wanted_dependency.bare_specifier.as_deref().unwrap_or("");
            if !bare.starts_with(self.prefix) {
                return Ok(None);
            }
            let name_ver = fake_name_ver();
            Ok(Some(ResolveResult {
                id: (&name_ver).into(),
                name_ver: Some(name_ver),
                latest: None,
                published_at: None,
                manifest: None,
                resolution: fake_resolution(),
                resolved_via: self.tag.to_string(),
                normalized_bare_specifier: None,
                alias: wanted_dependency.alias.clone(),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async move { Ok(Some(LatestInfo::default())) })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn empty_chain_returns_spec_not_supported_error() {
    let resolver = DefaultResolver::new(vec![]);
    let opts = ResolveOptions::default();
    let wd = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("1.2.3".to_string()),
        ..WantedDependency::default()
    };

    let err = resolver.resolve(&wd, &opts).await.expect_err("empty chain should error");
    let downcast = err
        .downcast_ref::<SpecNotSupportedByAnyResolverError>()
        .expect("error should be SpecNotSupportedByAnyResolverError");
    assert_eq!(downcast.specifier, "foo@1.2.3");
    assert_eq!(downcast.to_string(), r#""foo@1.2.3" isn't supported by any available resolver."#);
}

#[tokio::test(flavor = "current_thread")]
async fn first_claiming_resolver_wins() {
    let resolver = DefaultResolver::new(vec![
        Box::new(PrefixResolver { prefix: "git+", tag: "git" }),
        Box::new(PrefixResolver { prefix: "https://", tag: "tarball" }),
        Box::new(PrefixResolver { prefix: "", tag: "fallback" }),
    ]);
    let opts = ResolveOptions::default();

    let wd_git = WantedDependency {
        bare_specifier: Some("git+ssh://git@github.com/foo/bar".to_string()),
        ..WantedDependency::default()
    };
    let outcome = resolver.resolve(&wd_git, &opts).await.expect("git resolves");
    assert_eq!(outcome.resolved_via, "git", "first matching resolver wins, not the fallback");

    let wd_tarball = WantedDependency {
        bare_specifier: Some("https://example.com/foo.tgz".to_string()),
        ..WantedDependency::default()
    };
    let outcome = resolver.resolve(&wd_tarball, &opts).await.expect("tarball resolves");
    assert_eq!(outcome.resolved_via, "tarball");

    let wd_other = WantedDependency {
        bare_specifier: Some("1.2.3".to_string()),
        ..WantedDependency::default()
    };
    let outcome = resolver.resolve(&wd_other, &opts).await.expect("fallback resolves");
    assert_eq!(outcome.resolved_via, "fallback");
}

#[test]
fn spec_not_supported_renders_alias_and_bare_specifier() {
    let with_both = SpecNotSupportedByAnyResolverError::new(&WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("1.2.3".to_string()),
        ..WantedDependency::default()
    });
    assert_eq!(with_both.specifier, "foo@1.2.3");
    assert_eq!(with_both.to_string(), r#""foo@1.2.3" isn't supported by any available resolver."#);

    let bare_only = SpecNotSupportedByAnyResolverError::new(&WantedDependency {
        alias: None,
        bare_specifier: Some("git+ssh://example".to_string()),
        ..WantedDependency::default()
    });
    assert_eq!(bare_only.specifier, "git+ssh://example");
    assert_eq!(
        bare_only.to_string(),
        r#""git+ssh://example" isn't supported by any available resolver."#,
    );

    let alias_only = SpecNotSupportedByAnyResolverError::new(&WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: None,
        ..WantedDependency::default()
    });
    assert_eq!(alias_only.specifier, "foo");
    assert_eq!(alias_only.to_string(), r#""foo" isn't supported by any available resolver."#);

    let neither = SpecNotSupportedByAnyResolverError::new(&WantedDependency::default());
    assert_eq!(neither.specifier, "");
    assert_eq!(neither.to_string(), " isn't supported by any available resolver.");
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_latest_returns_none_when_chain_empty() {
    let resolver = DefaultResolver::new(vec![]);
    let opts = ResolveOptions::default();
    let query = LatestQuery { wanted_dependency: WantedDependency::default(), compatible: false };

    let info = resolver.resolve_latest(&query, &opts).await.expect("latest doesn't error");
    assert!(info.is_none(), "resolve_latest should fall through to None on an empty chain");
}
