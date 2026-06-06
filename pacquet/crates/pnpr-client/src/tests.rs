use super::{PnprClientError, VerifyError, parse_response};

/// A response header carrying verification violations is rebuilt into the
/// same `VerifyError` the local gate raises, so the CLI aborts with an
/// identical diagnostic code + breakdown.
#[test]
fn response_with_violations_rebuilds_a_verify_error() {
    let payload = br#"{"violations":[{"name":"@foo/no-deps","version":"1.0.0","code":"MINIMUM_RELEASE_AGE_VIOLATION","reason":"was published yesterday"}]}"#;
    let Err(PnprClientError::Verification(verify_err)) = parse_response(payload) else {
        panic!("expected a Verification error");
    };
    assert!(
        matches!(verify_err, VerifyError::MinimumReleaseAgeViolation { .. }),
        "got {verify_err:?}",
    );
    assert!(verify_err.to_string().contains("@foo/no-deps@1.0.0"), "got {verify_err}");
}

/// A lone `TARBALL_URL_MISMATCH` maps to the generic envelope — matching
/// `VerifyError::from_rendered`'s handling of a code with no dedicated
/// variant.
#[test]
fn tarball_mismatch_maps_to_the_generic_envelope() {
    let payload = br#"{"violations":[{"name":"acme","version":"1.0.0","code":"TARBALL_URL_MISMATCH","reason":"url mismatch"}]}"#;
    let Err(PnprClientError::Verification(verify_err)) = parse_response(payload) else {
        panic!("expected a Verification error");
    };
    assert!(
        matches!(verify_err, VerifyError::LockfileResolutionVerification { .. }),
        "got {verify_err:?}",
    );
}

/// A response with no lockfile and no violations is malformed, not a
/// silent success.
#[test]
fn response_without_a_lockfile_is_a_protocol_error() {
    let Err(PnprClientError::Protocol(_)) = parse_response(b"{}") else {
        panic!("expected a Protocol error");
    };
}
