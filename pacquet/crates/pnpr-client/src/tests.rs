use super::{Frame, PnprClientError, VerifyError, build_verify_error, parse_frame};

/// A `violations` frame is rebuilt into the same `VerifyError` the local
/// gate raises, so the CLI aborts with an identical diagnostic code +
/// breakdown.
#[test]
fn a_violations_frame_rebuilds_a_verify_error() {
    let line = br#"{"type":"violations","violations":[{"name":"@foo/no-deps","version":"1.0.0","code":"MINIMUM_RELEASE_AGE_VIOLATION","reason":"was published yesterday"}]}"#;
    let Frame::Violations { violations } = parse_frame(line).expect("frame parses") else {
        panic!("expected a violations frame");
    };
    let verify_err = build_verify_error(violations);
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
    let line = br#"{"type":"violations","violations":[{"name":"acme","version":"1.0.0","code":"TARBALL_URL_MISMATCH","reason":"url mismatch"}]}"#;
    let Frame::Violations { violations } = parse_frame(line).expect("frame parses") else {
        panic!("expected a violations frame");
    };
    let verify_err = build_verify_error(violations);
    assert!(
        matches!(verify_err, VerifyError::LockfileResolutionVerification { .. }),
        "got {verify_err:?}",
    );
}

/// A `package` frame carries the fetch hint fields verbatim.
#[test]
fn a_package_frame_parses_its_fetch_hint() {
    let line = br#"{"type":"package","id":"acme@1.0.0","name":"acme","version":"1.0.0","integrity":"sha512-abc","tarball":"https://r.test/acme/-/acme-1.0.0.tgz","unpackedSize":123456,"fileCount":42}"#;
    let Frame::Package { id, name, version, integrity, tarball, unpacked_size, file_count } =
        parse_frame(line).expect("frame parses")
    else {
        panic!("expected a package frame");
    };
    assert_eq!(id, "acme@1.0.0");
    assert_eq!(name, "acme");
    assert_eq!(version, "1.0.0");
    assert_eq!(integrity, "sha512-abc");
    assert_eq!(tarball, "https://r.test/acme/-/acme-1.0.0.tgz");
    assert_eq!(unpacked_size, Some(123456));
    assert_eq!(file_count, Some(42));
}

/// A `package` frame without `unpackedSize` / `fileCount` — an older
/// server, or a registry that never published the fields — still
/// parses.
#[test]
fn a_package_frame_without_dist_stats_parses() {
    let line = br#"{"type":"package","id":"acme@1.0.0","name":"acme","version":"1.0.0","integrity":"sha512-abc","tarball":"https://r.test/acme/-/acme-1.0.0.tgz"}"#;
    let Frame::Package { unpacked_size, file_count, .. } = parse_frame(line).expect("frame parses")
    else {
        panic!("expected a package frame");
    };
    assert_eq!(unpacked_size, None);
    assert_eq!(file_count, None);
}

/// A line with no `type` tag is malformed, not a silent success.
#[test]
fn an_untyped_frame_is_a_protocol_error() {
    let Err(PnprClientError::Protocol(_)) = parse_frame(b"{}") else {
        panic!("expected a Protocol error");
    };
}
