use super::{
    ResolveOptions,
    TrustPolicy,
    WantedDependency,
    assert_eq,
    build_resolver,
};
use crate::{
    PickPackageError,
    TrustCheckOptions,
    npm_resolver::registry_dependency::{
        PnprPackage,
        check_pnpr_trust_window,
    },
};
use jiff::{
    SignedDuration,
    Timestamp,
};
use pnpm_lockfile::{
    RegistryOptions,
    RegistryServerType,
};
use pnpm_resolving_resolver_base::{
    ResolutionPolicyOptions,
    Resolver,
    TrustIgnoreAfter,
};
use pnpm_version_policy::create_package_version_policy;
use std::{
    collections::BTreeMap,
    sync::Arc,
};

const PNPR: &str = "https://pnpr.example.com/";
const DAY: u64 = 24 * 60;

fn declared_pnpr(registry: &str) -> BTreeMap<String, RegistryOptions> {
    let options = RegistryOptions {
        server_type: Some(RegistryServerType::Pnpr),
        ..RegistryOptions::default()
    };
    BTreeMap::from([(registry.to_string(), options)])
}

/// The window the check refuses with, for a `trustPolicyIgnoreAfter` of
/// `ignore_after` minutes that counts back from a time `lag` behind the clock.
fn refused_window(ignore_after: u64, lag: Option<SignedDuration>, registry: &str) -> Option<u64> {
    let clock = pnpm_time::parse_rfc3339("2026-06-01T00:00:00Z").unwrap();
    let trust_check = TrustCheckOptions {
        trust_policy_ignore_after_minutes: Some(ignore_after),
        now: lag.map(|lag| clock - lag),
        ..TrustCheckOptions::default()
    };
    refusal(registry, &trust_check, clock)
}

fn refusal(registry: &str, trust_check: &TrustCheckOptions<'_>, clock: Timestamp) -> Option<u64> {
    let package = PnprPackage { registry, name: "acme" };
    match check_pnpr_trust_window(&declared_pnpr(PNPR), package, Some(trust_check), clock) {
        Ok(()) => None,
        Err(PickPackageError::UnsupportedTrustPolicyIgnoreAfter { minutes, .. }) => Some(minutes),
        Err(other) => panic!("unexpected error: {other}"),
    }
}

#[test]
fn only_a_package_exempt_at_every_version_skips_the_trust_window_refusal() {
    let clock = pnpm_time::parse_rfc3339("2026-06-01T00:00:00Z").unwrap();
    let refused_with = |exclude: &str| {
        let exclude = create_package_version_policy([exclude]).expect("policy");
        let trust_check = TrustCheckOptions {
            trust_policy_exclude: Some(&exclude),
            trust_policy_ignore_after_minutes: Some(DAY),
            now: Some(clock - jiff::SignedDuration::from_hours(696)),
            ..TrustCheckOptions::default()
        };
        refusal(PNPR, &trust_check, clock)
    };
    assert_eq!(refused_with("acme"), None, "exempt at every version");
    assert_eq!(refused_with("acme@1.0.0"), Some(30 * DAY), "exempt at one version");
    assert_eq!(refused_with("other"), Some(30 * DAY), "another package is exempt");
}

#[test]
fn a_trust_window_the_pnpr_document_cannot_date_is_refused() {
    let lag = |days: i64| Some(jiff::SignedDuration::from_hours(24 * days));
    assert_eq!(refused_window(DAY, lag(29), PNPR), Some(30 * DAY), "window reaches 30 days");
    assert_eq!(refused_window(DAY, lag(28), PNPR), None, "window stays under 30 days");
    assert_eq!(refused_window(DAY, lag(29), "https://registry.npmjs.org/"), None, "not pnpr");
    assert_eq!(refused_window(60 * DAY, None, PNPR), None, "counts back from the current time");
}

#[tokio::test]
async fn the_resolver_refuses_the_trust_window_before_reading_from_pnpr() {
    let mut server = mockito::Server::new_async().await;
    let unread = server
        .mock("GET", "/acme")
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);
    resolver.format.registry_options_by_url = Arc::new(declared_pnpr(&registry));

    let opts = ResolveOptions {
        policy: ResolutionPolicyOptions {
            trust_policy: Some(TrustPolicy::NoDowngrade),
            trust_policy_ignore_after: Some(TrustIgnoreAfter {
                minutes: DAY,
                now: Some(Timestamp::now() - jiff::SignedDuration::from_hours(720)),
            }),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wanted, &opts).await.expect_err("the window does not fit");
    eprintln!("resolve error: {err}");
    assert!(matches!(
        err.downcast_ref(),
        Some(PickPackageError::UnsupportedTrustPolicyIgnoreAfter { .. }),
    ));
    unread.assert_async().await;
}
