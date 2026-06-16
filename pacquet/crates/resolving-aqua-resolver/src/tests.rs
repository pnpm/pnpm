use std::sync::Arc;

use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};

use super::{AquaResolver, parse_aqua_specifier};

fn resolver() -> AquaResolver {
    AquaResolver::new(Arc::new(ThrottledClient::new_for_installs()))
}

#[tokio::test]
async fn returns_none_for_non_aqua_specifiers() {
    let wanted = WantedDependency {
        bare_specifier: Some("lodash@4.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn errors_in_offline_mode() {
    let mut resolver = resolver();
    resolver.offline = true;
    let wanted = WantedDependency {
        bare_specifier: Some("aqua:BurntSushi/ripgrep".to_string()),
        ..WantedDependency::default()
    };
    let error = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();
    assert!(error.to_string().contains("offline"), "unexpected error: {error}");
}

#[tokio::test]
async fn errors_for_invalid_specifier_without_owner_repo() {
    let wanted = WantedDependency {
        bare_specifier: Some("aqua:ripgrep".to_string()),
        ..WantedDependency::default()
    };
    let error = resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();
    assert!(error.to_string().contains("Expected format"), "unexpected error: {error}");
}

#[test]
fn parses_owner_repo_without_version() {
    let parsed = parse_aqua_specifier("aqua:BurntSushi/ripgrep").unwrap();
    assert_eq!(parsed.owner, "BurntSushi");
    assert_eq!(parsed.repo, "ripgrep");
    assert_eq!(parsed.version_spec, None);
}

#[test]
fn parses_owner_repo_with_version() {
    let parsed = parse_aqua_specifier("aqua:junegunn/fzf@v0.57.0").unwrap();
    assert_eq!(parsed.owner, "junegunn");
    assert_eq!(parsed.repo, "fzf");
    assert_eq!(parsed.version_spec.as_deref(), Some("v0.57.0"));
}

#[test]
fn rejects_specifier_without_slash() {
    assert!(parse_aqua_specifier("aqua:ripgrep").is_err());
}
