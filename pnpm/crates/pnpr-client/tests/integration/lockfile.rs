use super::{
    PnprClient, PnprClientError, TestRegistry, VerifyLockfileOptions, deps, options,
    register_token, registry_upstream, start_pnpr, start_pnpr_with_upstreams_at,
};

/// The streaming API surfaces each resolved tarball as a `package`
/// frame *before* the terminal `done` frame carrying the lockfile, and
/// every streamed package appears in the final lockfile. This is the
/// overlap lever: the caller can begin fetching each tarball the moment
/// its frame arrives, while the server is still resolving.
#[tokio::test]
async fn streams_resolved_packages_before_the_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let mut streamed: Vec<String> = Vec::new();
    let outcome = client
        .resolve_streaming(
            options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])),
            |pkg| {
                assert!(!pkg.integrity.is_empty(), "a package frame carries an integrity");
                assert!(pkg.tarball.starts_with("http"), "a package frame carries a tarball URL");
                assert_eq!(pkg.id, format!("{}@{}", pkg.name, pkg.version), "id is name@version");
                streamed.push(pkg.id);
            },
        )
        .await
        .expect("streaming resolve should succeed");

    assert!(!streamed.is_empty(), "at least one package frame streams before `done`");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    for id in &streamed {
        assert!(
            packages.keys().any(|key| key.to_string() == *id),
            "streamed package {id} should appear in the resolved lockfile, got: {:?}",
            packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
        );
    }
}

#[tokio::test]
async fn verifies_and_accepts_a_clean_input_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    // A first install with no lockfile produces a valid resolved one.
    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Sending it back as the input lockfile makes the server verify it
    // under the (default, policy-free) client policy before resolving;
    // a clean lockfile passes and the install succeeds.
    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    let second = client.resolve(opts).await.expect("verified-input install should succeed");
    assert!(second.lockfile.packages.is_some(), "resolution still produced a lockfile");
}

#[tokio::test]
async fn rejects_an_input_lockfile_that_violates_the_clients_policy() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Re-send the same lockfile under a ~100-year minimumReleaseAge: no
    // real publish time can satisfy it, so the server rejects the input
    // lockfile and the client rebuilds the identical `VerifyError`.
    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;

    let Err(PnprClientError::Verification(verify_err)) = client.resolve(opts).await else {
        panic!("expected a verification error rejecting the input lockfile");
    };
    assert!(
        verify_err.to_string().contains("minimumReleaseAge"),
        "expected a minimumReleaseAge breakdown, got: {verify_err}",
    );
}

#[tokio::test]
async fn verify_lockfile_endpoint_accepts_a_clean_input_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile);
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&opts).expect("lockfile is present");

    client.verify_lockfile(verify_opts).await.expect("lockfile should verify");
}

#[tokio::test]
async fn verify_lockfile_endpoint_rejects_policy_violation() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile);
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&opts).expect("lockfile is present");

    let Err(PnprClientError::Verification(verify_err)) = client.verify_lockfile(verify_opts).await
    else {
        panic!("expected a verification error rejecting the input lockfile");
    };
    assert!(
        verify_err.to_string().contains("minimumReleaseAge"),
        "expected a minimumReleaseAge breakdown, got: {verify_err}",
    );
}

/// The verification fan-out fetches each entry's packument, so a gated
/// package verifies only when the pnpr server has an upstream for the
/// registry — and fails closed against a server without one. Each verify
/// targets a fresh pnpr so neither the whole-lockfile verdict cache nor the
/// metadata mirror warmed by an earlier call can satisfy it without
/// exercising the upstream. The resolve and aliased-verify instances share a
/// `public_url` so the verifier can reverse the lockfile's `/~<name>/`
/// tarball URLs back to upstream — what a real single-pnpr deployment does.
#[tokio::test]
async fn verify_lockfile_endpoint_uses_upstreams() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-verifier").await;
    let shared_public_url = "http://pnpr.verify.test";

    let (resolve_pnpr_url, resolve_auth, _resolve_storage) = start_pnpr_with_upstreams_at(
        shared_public_url,
        vec![registry_upstream(&registry.url(), &token)],
    )
    .await;
    let mut resolve_opts =
        options(&registry.url(), &resolve_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let first = PnprClient::new(resolve_pnpr_url)
        .resolve(resolve_opts.clone())
        .await
        .expect("aliased install");

    // An active policy makes the verifier fetch the gated packument.
    resolve_opts.lockfile = Some(first.lockfile);
    resolve_opts.minimum_release_age = Some(1);
    resolve_opts.minimum_release_age_ignore_missing_time = false;

    // A fresh pnpr that carries the upstream verifies the gated entry.
    let (aliased_pnpr_url, aliased_auth, _aliased_storage) = start_pnpr_with_upstreams_at(
        shared_public_url,
        vec![registry_upstream(&registry.url(), &token)],
    )
    .await;
    let mut aliased_opts = resolve_opts.clone();
    aliased_opts.authorization = Some(aliased_auth);
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&aliased_opts).expect("lockfile is present");
    PnprClient::new(aliased_pnpr_url)
        .verify_lockfile(verify_opts)
        .await
        .expect("the upstream should let the gated entry verify");

    // A pnpr without the upstream has no credential to select, so the gated
    // entry's metadata fetch must fail closed.
    let (plain_pnpr_url, plain_auth, _plain_storage) = start_pnpr(&registry.url()).await;
    let mut plain_opts = resolve_opts.clone();
    plain_opts.authorization = Some(plain_auth);
    let plain_verify_opts =
        VerifyLockfileOptions::from_resolve_options(&plain_opts).expect("lockfile is present");
    assert!(
        PnprClient::new(plain_pnpr_url).verify_lockfile(plain_verify_opts).await.is_err(),
        "without an upstream the gated entry's metadata fetch must fail closed",
    );
}

#[tokio::test]
async fn trust_lockfile_makes_the_server_skip_verification() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Same policy that `rejects_an_input_lockfile_that_violates_the_clients_policy`
    // trips on, but with the client's `trustLockfile` opt-out set: the
    // server must skip the verify gate and resolve normally, matching the
    // local `--trust-lockfile` path.
    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;
    opts.trust_lockfile = true;

    let outcome = client.resolve(opts).await.expect("trustLockfile should skip verification");
    assert!(outcome.lockfile.packages.is_some(), "install still resolved a lockfile");
}
