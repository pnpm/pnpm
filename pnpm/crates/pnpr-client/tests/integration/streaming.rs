use super::{
    PnprClient, PnprClientError, TestRegistry, deps, options, register_token, registry_upstream,
    start_pnpr, start_pnpr_with_upstreams,
};

/// End-to-end: the test registry gates `@pnpm.e2e/needs-auth` behind
/// `$authenticated`. The client never forwards its own credentials, so
/// resolving it works only when the pnpr server is configured with an
/// upstream for that registry that the caller is
/// authorized to use.
#[tokio::test]
async fn an_upstream_resolves_a_private_package() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-forwarder").await;
    let (pnpr_url, pnpr_auth, _storage) =
        start_pnpr_with_upstreams(vec![registry_upstream(&registry.url(), &token)]).await;

    let client = PnprClient::new(pnpr_url);

    let opts = options(&registry.url(), &pnpr_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let outcome = client.resolve(opts).await.expect("the upstream should resolve it");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@pnpm.e2e/needs-auth@1.0.0")),
        "lockfile should contain the authed package, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}

/// The same install against a server with no matching upstream
/// fails: the client forwards no credential and pnpr has none to select,
/// so the gated packument can only be fetched anonymously — which the
/// registry refuses.
#[tokio::test]
async fn a_private_package_fails_without_an_upstream() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let opts = options(&registry.url(), &pnpr_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let Err(PnprClientError::Server(message)) = client.resolve(opts).await else {
        panic!("expected the gated install to fail with a server error");
    };
    assert!(message.contains("401"), "expected an auth denial without an upstream, got: {message}");
}

/// An unknown route (no upstream, no public rule) has no managed credential,
/// so it was resolved anonymously and pnpr mints no gateway URL: the
/// resolution keeps its registry resolution, and the client fetches the
/// tarball directly from the upstream the same way pnpr did.
#[tokio::test]
async fn unknown_route_keeps_its_upstream_tarball_url() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let outcome = PnprClient::new(pnpr_url)
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("install should succeed");
    let lockfile = serde_json::to_value(&outcome.lockfile).expect("lockfile serializes");
    let resolution = &lockfile["packages"]["@foo/no-deps@1.0.0"]["resolution"];

    // No pnpr gateway URL is minted; the entry stays integrity-only (its URL
    // is reconstructed from the client's configured registry).
    assert!(
        resolution.get("tarball").is_none(),
        "unknown route should stay integrity-only, got: {resolution}",
    );

    // The tarball is fetchable directly from the upstream registry.
    let direct = reqwest::get(format!("{}@foo/no-deps/-/no-deps-1.0.0.tgz", registry.url()))
        .await
        .expect("direct tarball request");
    assert!(direct.status().is_success(), "registry returned {}", direct.status());
}
