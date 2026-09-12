use super::{
    PkgName, ResolutionVerification, TrustPolicy, assert_eq, create_npm_resolution_verifier,
    create_package_version_policy, ctx, default_opts, now_at, registry_resolution,
    stable_trust_packument, time_free_trust_packument, trust_downgrade_packument,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

/// `trust_policy = Off` keeps the trust check inactive (same tripwire
/// rationale as the age-check test above).
#[tokio::test]
async fn trust_off_keeps_trust_check_inactive() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.trust_policy = Some(TrustPolicy::Off);
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn trust_downgrade_publisher_to_provenance_fails() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(trust_downgrade_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TRUST_DOWNGRADE");
    assert!(reason.contains("trust downgrade"), "got reason: {reason}");
}

#[tokio::test]
async fn trust_downgrade_pass_when_no_weaker_evidence() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(stable_trust_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn trust_time_free_packument_fails_closed_by_default() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(time_free_trust_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TRUST_DOWNGRADE");
    assert!(reason.contains(r#"missing the "time" field"#), "got reason: {reason}");
}

/// The same registry deficiency the age check already tolerates under
/// this opt-in: with no `time` map there is no publish order for the
/// downgrade walk to read, so the verifier passes the entry rather than
/// locking the user out of a registry that never serves the field.
#[tokio::test]
async fn trust_time_free_packument_passes_when_ignored() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(time_free_trust_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.ignore_missing_time_field = true;
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn trust_downgrade_still_reported_when_ignored_and_time_is_complete() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(trust_downgrade_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.ignore_missing_time_field = true;
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TRUST_DOWNGRADE");
    assert!(reason.contains("trust downgrade"), "got reason: {reason}");
}

/// Dropping the missing-time tolerance invalidates a cached run that
/// may have waved entries through on it; adding the tolerance keeps a
/// stricter cached run trustworthy, since it accepted a subset of what
/// today's policy accepts.
#[test]
fn can_trust_past_check_tracks_ignore_missing_time_field() {
    let mut tolerant_opts = default_opts("https://registry.example/");
    tolerant_opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    tolerant_opts.ignore_missing_time_field = true;
    let tolerant = create_npm_resolution_verifier(tolerant_opts);

    let mut strict_opts = default_opts("https://registry.example/");
    strict_opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    let strict = create_npm_resolution_verifier(strict_opts);

    assert!(!strict.can_trust_past_check(tolerant.policy()));
    assert!(tolerant.can_trust_past_check(strict.policy()));
}

/// A record written before the field existed reads as intolerant, which
/// is the safe direction: it cannot have passed anything today's
/// stricter policy would reject.
#[test]
fn can_trust_past_check_reads_a_missing_tolerance_field_as_intolerant() {
    let mut opts = default_opts("https://registry.example/");
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = verifier.policy().clone();
    cached.remove("minimumReleaseAgeIgnoreMissingTime");
    assert!(verifier.can_trust_past_check(&cached));
}

/// Repointing an alias is the change that matters: the alias set is
/// identical, so a digest over alias names alone would still trust the
/// cached policy and reuse resolutions fetched from the old host.
#[test]
fn can_trust_past_check_rejects_changed_named_registry_mapping() {
    let mut opts = default_opts("https://registry.example/");
    opts.registries_by_prefix
        .insert("work".to_string(), "https://registry.work.example/".to_string());
    let verifier = create_npm_resolution_verifier(opts);
    let cached = verifier.policy().clone();
    let mut changed_opts = default_opts("https://registry.example/");
    changed_opts
        .registries_by_prefix
        .insert("work".to_string(), "https://other.example/".to_string());
    let changed = create_npm_resolution_verifier(changed_opts);

    // Pins that the rejection below comes from the URL change and not from
    // something incidental to how the policy is built.
    assert!(verifier.can_trust_past_check(&cached));
    assert!(!changed.can_trust_past_check(&cached));
}

/// A cache record that predates the tarball-URL binding rule (no
/// `tarballUrlBinding` marker) can't be trusted to have enforced it,
/// so it's rejected and forces a re-verification.
#[test]
fn can_trust_past_check_rejects_missing_tarball_url_binding() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24);
    let verifier = create_npm_resolution_verifier(opts);

    // Otherwise-compatible cached policy, but without the binding marker.
    let mut cached = serde_json::Map::new();
    cached.insert("integrityRequired".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), (60 * 24 * 7).into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Same rule for the missing-integrity check: a record written before
/// the rule existed can't prove it rejected unverifiable tarballs, so
/// the lockfile is re-verified rather than trusted.
#[test]
fn can_trust_past_check_rejects_missing_integrity_required() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24);
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), (60 * 24 * 7).into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Any drift in the exclude list invalidates the cached run, even
/// when the drift would have been more permissive (an extra entry):
/// the check is a stricter-than-necessary identity comparison.
#[test]
fn can_trust_past_check_rejects_changed_exclude_list() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24);
    opts.minimum_release_age_exclude_patterns = vec!["acme".to_string()];
    opts.minimum_release_age_exclude =
        Some(create_package_version_policy(["acme".to_string()]).expect("policy"));
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
    cached.insert("integrityRequired".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), (60 * 24).into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Switching trust policy on or off invalidates the cached run.
#[test]
fn can_trust_past_check_rejects_changed_trust_policy() {
    let mut opts = default_opts("https://registry.example/");
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
    cached.insert("integrityRequired".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), 0.into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Changing `trustPolicyIgnoreAfter` (or going from set to unset)
/// invalidates the cache.
#[test]
fn can_trust_past_check_rejects_changed_ignore_after() {
    let mut opts = default_opts("https://registry.example/");
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.trust_policy_ignore_after = Some(60 * 24 * 14);
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
    cached.insert("integrityRequired".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), 0.into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::String("no-downgrade".into()));
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}
