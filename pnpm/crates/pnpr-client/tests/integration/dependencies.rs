use super::{BTreeMap, PnprClient, TestRegistry, deps, options, start_pnpr};

/// Optional dependencies must reach the server in the request, not be
/// silently dropped, so the resolved lockfile includes their edges.
#[tokio::test]
async fn forwards_optional_dependencies() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let mut opts = options(&registry.url(), &pnpr_auth, BTreeMap::new());
    opts.optional_dependencies = deps([("@foo/no-deps", "1.0.0")]);

    let outcome = client.resolve(opts).await.expect("install should succeed");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "the optional dependency should be resolved into the lockfile, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}
