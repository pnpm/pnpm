use super::{PnprClientError, VerifyError, parse_install_response};

/// A structured `E` line (the server's verification rejection) is
/// rebuilt into the same `VerifyError` the local gate raises, so the CLI
/// aborts with an identical diagnostic code + breakdown.
#[test]
fn e_line_with_violations_rebuilds_a_verify_error() {
    let ndjson = "E\t{\"violations\":[{\"name\":\"@foo/no-deps\",\"version\":\"1.0.0\",\"code\":\"MINIMUM_RELEASE_AGE_VIOLATION\",\"reason\":\"was published yesterday\"}]}\n";
    let Err(PnprClientError::Verification(verify_err)) = parse_install_response(ndjson) else {
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
    let ndjson = "E\t{\"violations\":[{\"name\":\"acme\",\"version\":\"1.0.0\",\"code\":\"TARBALL_URL_MISMATCH\",\"reason\":\"url mismatch\"}]}\n";
    let Err(PnprClientError::Verification(verify_err)) = parse_install_response(ndjson) else {
        panic!("expected a Verification error");
    };
    assert!(
        matches!(verify_err, VerifyError::LockfileResolutionVerification { .. }),
        "got {verify_err:?}",
    );
}

/// A plain `E` error line (no `violations`) stays a generic server error
/// rather than a verification error.
#[test]
fn e_line_with_plain_error_is_a_server_error() {
    let ndjson = "E\t{\"error\":\"resolution failed\"}\n";
    let Err(PnprClientError::Server(message)) = parse_install_response(ndjson) else {
        panic!("expected a Server error");
    };
    assert_eq!(message, "resolution failed");
}
