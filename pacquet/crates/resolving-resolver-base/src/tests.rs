use pacquet_lockfile::{LockfileResolution, PkgName, PkgNameVer, RegistryResolution};
use ssri::Integrity;

use crate::{
    DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT, LatestInfo, LatestQuery,
    ResolutionPolicyViolation, ResolutionVerification, ResolutionVerifier, ResolveOptions,
    ResolveResult, Resolver, UpdateBehavior, VerifyCtx, WantedDependency,
};

fn fake_resolution() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
            .parse::<Integrity>()
            .expect("parse fake integrity"),
    })
}

/// [`ResolutionVerification::Err`] carries the verifier-supplied code
/// and reason verbatim; the runner pulls these out to compose the
/// install-level error breakdown. `code` is a `&'static str` so the
/// per-policy constants can flow through without allocation.
#[test]
fn resolution_verification_err_round_trip() {
    let verification = ResolutionVerification::Err {
        code: "MINIMUM_RELEASE_AGE_VIOLATION",
        reason: "was published yesterday".to_string(),
    };
    match verification {
        ResolutionVerification::Err { code, reason } => {
            assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");
            assert_eq!(reason, "was published yesterday");
        }
        ResolutionVerification::Ok => panic!("expected Err"),
    }
}

/// [`ResolutionPolicyViolation`] is the data shape the runner
/// aggregates and sorts by `name@version`. Constructing one with a
/// real [`PkgName`] proves the type composes with `pacquet_lockfile`.
#[test]
fn resolution_policy_violation_carries_pkg_name_and_resolution() {
    let violation = ResolutionPolicyViolation {
        name: "lodash".parse().expect("parse PkgName"),
        version: "4.17.21".to_string(),
        resolution: fake_resolution(),
        code: "MINIMUM_RELEASE_AGE_VIOLATION",
        reason: "was published yesterday".to_string(),
    };
    assert_eq!(violation.name.to_string(), "lodash");
    assert_eq!(violation.version, "4.17.21");
    assert_eq!(violation.code, "MINIMUM_RELEASE_AGE_VIOLATION");
}

/// Stand-in verifier that demonstrates the trait is implementable
/// with the manual boxed-future return type. Returns `Err` to prove
/// the err arm round-trips through the boxed future.
struct StubVerifier {
    policy: serde_json::Map<String, serde_json::Value>,
}

impl ResolutionVerifier for StubVerifier {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> crate::VerifyFuture<'a> {
        Box::pin(async move {
            ResolutionVerification::Err { code: "STUB", reason: "stub fails by design".to_string() }
        })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(
        &self,
        cached_policy: &serde_json::Map<String, serde_json::Value>,
    ) -> bool {
        cached_policy.get("stub").and_then(serde_json::Value::as_bool).unwrap_or(false)
    }
}

/// The trait is implementable and dispatch-able from a `&dyn` slot,
/// which is the shape the runner stores its verifier list in.
#[tokio::test(flavor = "current_thread")]
async fn resolution_verifier_dispatches_through_dyn() {
    let mut policy = serde_json::Map::new();
    policy.insert("stub".to_string(), serde_json::Value::Bool(true));
    let verifier: Box<dyn ResolutionVerifier> = Box::new(StubVerifier { policy });

    let name: PkgName = "lodash".parse().unwrap();
    let resolution = fake_resolution();
    let outcome = verifier.verify(&resolution, VerifyCtx { name: &name, version: "4.17.21" }).await;
    assert_eq!(
        outcome,
        ResolutionVerification::Err { code: "STUB", reason: "stub fails by design".to_string() },
    );

    let mut cached = serde_json::Map::new();
    cached.insert("stub".to_string(), serde_json::Value::Bool(true));
    assert!(verifier.can_trust_past_check(&cached));

    cached.insert("stub".to_string(), serde_json::Value::Bool(false));
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Selector weight ordering is part of the public contract: existing
/// pins must always outrank direct-dep matches so that adding a fresh
/// range doesn't churn the lockfile.
const _: () = assert!(EXISTING_VERSION_SELECTOR_WEIGHT > DIRECT_DEP_SELECTOR_WEIGHT);

/// [`UpdateBehavior::default`] mirrors upstream's `update?: false`
/// default — keep the lockfile pin.
#[test]
fn update_behavior_defaults_off() {
    assert_eq!(UpdateBehavior::default(), UpdateBehavior::Off);
}

/// Stand-in resolver that demonstrates the [`Resolver`] trait is
/// implementable with the manual boxed-future return type, and that
/// the chain shape `Vec<Box<dyn Resolver>>` round-trips. Claims any
/// wanted dependency whose alias starts with `claim:`.
struct StubResolver;

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> crate::ResolveFuture<'a> {
        Box::pin(async move {
            let alias = wanted_dependency.alias.as_deref().unwrap_or("");
            if !alias.starts_with("claim:") {
                return Ok(None);
            }
            let name_ver: PkgNameVer = "lodash@4.17.21".parse().expect("parse fake PkgNameVer");
            Ok(Some(ResolveResult {
                id: (&name_ver).into(),
                name_ver: Some(name_ver),
                latest: None,
                published_at: None,
                manifest: None,
                resolution: fake_resolution(),
                resolved_via: "stub".to_string(),
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
    ) -> crate::ResolveLatestFuture<'a> {
        Box::pin(async move { Ok(Some(LatestInfo::default())) })
    }
}

/// The [`Resolver`] trait dispatches through a `Box<dyn Resolver>` slot
/// (the shape the default-resolver chain stores) and the `Ok(None)` /
/// `Ok(Some(_))` discriminator round-trips through the boxed future.
#[tokio::test(flavor = "current_thread")]
async fn resolver_dispatches_through_dyn_and_returns_none_when_unclaimed() {
    let resolver: Box<dyn Resolver> = Box::new(StubResolver);
    let opts = ResolveOptions::default();

    let unclaimed = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("1.2.3".to_string()),
        ..WantedDependency::default()
    };
    let outcome = resolver.resolve(&unclaimed, &opts).await.expect("resolve unclaimed");
    assert!(outcome.is_none(), "resolver should defer when it doesn't claim the dep");

    let claimed = WantedDependency {
        alias: Some("claim:foo".to_string()),
        bare_specifier: Some("1.2.3".to_string()),
        ..WantedDependency::default()
    };
    let outcome = resolver.resolve(&claimed, &opts).await.expect("resolve claimed");
    let result = outcome.expect("resolver should claim the dep");
    assert_eq!(result.resolved_via, "stub");
    assert_eq!(result.alias.as_deref(), Some("claim:foo"));
    assert_eq!(result.id.to_string(), "lodash@4.17.21");
}
