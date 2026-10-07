use super::origins_redirect_guard;

#[test]
fn origins_redirect_guard_allows_same_origin() {
    let guard = origins_redirect_guard(["https://registry.npmjs.org/"]);
    let target = reqwest::Url::parse("https://registry.npmjs.org/lodash").unwrap();
    assert!(guard(&target));
}

#[test]
fn origins_redirect_guard_normalizes_default_port() {
    let guard = origins_redirect_guard(["https://registry.npmjs.org:443/"]);
    let target = reqwest::Url::parse("https://registry.npmjs.org/foo").unwrap();
    assert!(guard(&target));

    let guard_implicit = origins_redirect_guard(["https://registry.npmjs.org/"]);
    let target_explicit = reqwest::Url::parse("https://registry.npmjs.org:443/foo").unwrap();
    assert!(guard_implicit(&target_explicit));
}

#[test]
fn origins_redirect_guard_blocks_different_host() {
    let guard = origins_redirect_guard(["https://registry.npmjs.org/"]);
    let target = reqwest::Url::parse("https://evil.com/pkg").unwrap();
    assert!(!guard(&target));
}

#[test]
fn origins_redirect_guard_blocks_different_scheme() {
    let guard = origins_redirect_guard(["https://registry.npmjs.org/"]);
    let target = reqwest::Url::parse("http://registry.npmjs.org/pkg").unwrap();
    assert!(!guard(&target));
}

#[test]
fn origins_redirect_guard_blocks_different_port() {
    let guard = origins_redirect_guard(["https://registry.npmjs.org:8443/"]);
    let target = reqwest::Url::parse("https://registry.npmjs.org:9443/pkg").unwrap();
    assert!(!guard(&target));
}

#[test]
fn origins_redirect_guard_supports_multiple_origins() {
    let guard =
        origins_redirect_guard(["https://registry.npmjs.org/", "https://npm.pkg.github.com/"]);
    assert!(guard(&reqwest::Url::parse("https://registry.npmjs.org/a").unwrap()));
    assert!(guard(&reqwest::Url::parse("https://npm.pkg.github.com/b").unwrap()));
    assert!(!guard(&reqwest::Url::parse("https://other.com/c").unwrap()));
}
