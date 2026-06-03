use super::{PnprClientError, VerifyError, parse_framed_response};

/// Frame a JSON payload as a single `E` (error/violation) frame —
/// `[b'E'][u32 BE len][json]`.
fn error_frame(json: &str) -> Vec<u8> {
    let mut frame = vec![b'E'];
    frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
    frame.extend_from_slice(json.as_bytes());
    frame
}

/// An `E` frame carrying verification violations is rebuilt into the same
/// `VerifyError` the local gate raises, so the CLI aborts with an
/// identical diagnostic code + breakdown.
#[test]
fn violation_frame_rebuilds_a_verify_error() {
    let frame = error_frame(
        r#"{"violations":[{"name":"@foo/no-deps","version":"1.0.0","code":"MINIMUM_RELEASE_AGE_VIOLATION","reason":"was published yesterday"}]}"#,
    );
    let Err(PnprClientError::Verification(verify_err)) = parse_framed_response(&frame) else {
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
    let frame = error_frame(
        r#"{"violations":[{"name":"acme","version":"1.0.0","code":"TARBALL_URL_MISMATCH","reason":"url mismatch"}]}"#,
    );
    let Err(PnprClientError::Verification(verify_err)) = parse_framed_response(&frame) else {
        panic!("expected a Verification error");
    };
    assert!(
        matches!(verify_err, VerifyError::LockfileResolutionVerification { .. }),
        "got {verify_err:?}",
    );
}

/// A plain `E` error frame (no `violations`) stays a generic server error.
#[test]
fn plain_error_frame_is_a_server_error() {
    let Err(PnprClientError::Server(message)) =
        parse_framed_response(&error_frame(r#"{"error":"resolution failed"}"#))
    else {
        panic!("expected a Server error");
    };
    assert_eq!(message, "resolution failed");
}

/// A stream with no `L` frame is a malformed response, not a silent
/// success.
#[test]
fn stream_without_a_lockfile_is_a_protocol_error() {
    let Err(PnprClientError::Protocol(_)) = parse_framed_response(&[]) else {
        panic!("expected a Protocol error");
    };
}
