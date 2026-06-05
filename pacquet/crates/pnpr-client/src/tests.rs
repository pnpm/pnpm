use std::collections::HashMap;

use pacquet_lockfile::{ComVer, Lockfile, LockfileVersion};

use super::{PnprClientError, VerifyError, parse_inline_response};

/// Frame a JSON header into a complete inline install payload with an
/// empty file section (the `{}` prefix plus the end-of-stream marker),
/// matching what the server sends when there are no files to inline.
fn inline_payload(header_json: &str) -> Vec<u8> {
    let header = header_json.as_bytes();
    let mut payload = Vec::new();
    payload.extend_from_slice(&(header.len() as u32).to_be_bytes());
    payload.extend_from_slice(header);
    payload.extend_from_slice(&2u32.to_be_bytes());
    payload.extend_from_slice(b"{}");
    payload.extend_from_slice(&[0u8; 64]);
    payload
}

/// A header carrying verification violations is rebuilt into the same
/// `VerifyError` the local gate raises, so the CLI aborts with an
/// identical diagnostic code + breakdown.
#[test]
fn header_with_violations_rebuilds_a_verify_error() {
    let payload = inline_payload(
        r#"{"violations":[{"name":"@foo/no-deps","version":"1.0.0","code":"MINIMUM_RELEASE_AGE_VIOLATION","reason":"was published yesterday"}]}"#,
    );
    let Err(PnprClientError::Verification(verify_err)) = parse_inline_response(&payload, None)
    else {
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
    let payload = inline_payload(
        r#"{"violations":[{"name":"acme","version":"1.0.0","code":"TARBALL_URL_MISMATCH","reason":"url mismatch"}]}"#,
    );
    let Err(PnprClientError::Verification(verify_err)) = parse_inline_response(&payload, None)
    else {
        panic!("expected a Verification error");
    };
    assert!(
        matches!(verify_err, VerifyError::LockfileResolutionVerification { .. }),
        "got {verify_err:?}",
    );
}

/// A header with no lockfile and no violations is a malformed response,
/// not a silent success.
#[test]
fn header_without_a_lockfile_is_a_protocol_error() {
    let payload = inline_payload("{}");
    let Err(PnprClientError::Protocol(_)) = parse_inline_response(&payload, None) else {
        panic!("expected a Protocol error");
    };
}

#[test]
fn use_input_lockfile_reuses_the_clients_lockfile() {
    let input = Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        ignored_optional_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    };
    let payload = inline_payload(r#"{"useInputLockfile":true}"#);

    let parsed = parse_inline_response(&payload, Some(&input)).expect("parse response");

    assert!(parsed.reused_input_lockfile);
    assert_eq!(parsed.lockfile, input);
}
