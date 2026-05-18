use pacquet_lockfile::{LockfileResolution, PkgName, RegistryResolution};
use ssri::Integrity;

use crate::{ResolutionPolicyViolation, ResolutionVerification, ResolutionVerifier, VerifyCtx};

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
    let v = ResolutionVerification::Err {
        code: "MINIMUM_RELEASE_AGE_VIOLATION",
        reason: "was published yesterday".to_string(),
    };
    match v {
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
        cached_policy.get("stub").and_then(|value| value.as_bool()).unwrap_or(false)
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
