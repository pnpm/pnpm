use pnpr_error::RegistryError;
use serde_json::{Value, json};

use super::{
    CredentialLimits, MAX_TOKEN_TIMESTAMP_MILLIS, MILLIS_PER_DAY, MILLIS_PER_SECOND, TokenAccess,
    TokenExpiry,
    parse_and_validate_token_request,
};

const FIXED_NOW: u64 = 1_700_000_000_000;

fn parse_json(value: &Value) -> Result<super::ValidatedTokenRequest, RegistryError> {
    let bytes = serde_json::to_vec(value).expect("serialize test payload");
    parse_and_validate_token_request(&bytes, FIXED_NOW)
}

fn assert_bad_request(value: &Value) -> String {
    match parse_json(value) {
        Err(RegistryError::BadRequest { reason }) => reason,
        Ok(_) => panic!("expected BadRequest for {value}, got Ok"),
        Err(other) => panic!("expected BadRequest for {value}, got {other:?}"),
    }
}

#[test]
fn empty_body_and_empty_object_default_to_read_write_without_expiry() {
    let from_empty = parse_and_validate_token_request(b"", FIXED_NOW).unwrap();
    assert_eq!(from_empty.access, TokenAccess::ReadWrite);
    assert_eq!(from_empty.expiry, TokenExpiry::Never);
    assert!(from_empty.password().is_none());

    let from_obj = parse_json(&json!({})).unwrap();
    assert_eq!(from_obj.access, TokenAccess::ReadWrite);
    assert_eq!(from_obj.expiry, TokenExpiry::Never);
}

#[test]
fn maps_packages_and_scopes_permission_and_classic_readonly() {
    let npm_ro = parse_json(&json!({
        "name": null,
        "password": "secret",
        "packages_and_scopes_permission": "read-only",
        "packages_all": true,
        "description": "ci reader",
        "bypass_2fa": false,
    }))
    .unwrap();
    assert_eq!(npm_ro.access, TokenAccess::ReadOnly);

    let npm_rw = parse_json(&json!({
        "name": "publisher",
        "password": "secret",
        "packages_and_scopes_permission": "read-write",
        "packages_all": true,
        "bypass_2fa": true,
    }))
    .unwrap();
    assert_eq!(npm_rw.access, TokenAccess::ReadWrite);

    let matching_ro = parse_json(&json!({
        "readonly": true,
        "packages_and_scopes_permission": "read-only",
    }))
    .unwrap();
    assert_eq!(matching_ro.access, TokenAccess::ReadOnly);

    let matching_rw = parse_json(&json!({
        "readonly": false,
        "packages_and_scopes_permission": "read-write",
    }))
    .unwrap();
    assert_eq!(matching_rw.access, TokenAccess::ReadWrite);
}

#[test]
fn rejects_unsupported_or_conflicting_permission_values() {
    let stage_reason = assert_bad_request(&json!({
        "packages_and_scopes_permission": "read-write-stage-only",
    }));
    assert!(stage_reason.contains("read-write-stage-only"));

    let no_access_reason = assert_bad_request(&json!({
        "packages_and_scopes_permission": "no-access",
    }));
    assert!(no_access_reason.contains("no-access"));

    let unknown_reason = assert_bad_request(&json!({
        "packages_and_scopes_permission": "publish-only",
    }));
    assert!(unknown_reason.contains("publish-only"));

    let conflict_one = assert_bad_request(&json!({
        "readonly": true,
        "packages_and_scopes_permission": "read-write",
    }));
    assert!(conflict_one.contains("conflicting"));

    let conflict_two = assert_bad_request(&json!({
        "readonly": false,
        "packages_and_scopes_permission": "read-only",
    }));
    assert!(conflict_two.contains("conflicting"));
}

#[test]
fn rejects_granular_package_scope_org_and_unknown_fields() {
    for payload in [
        json!({ "packages": ["@acme/pkg"] }),
        json!({ "packages": [] }),
        json!({ "packages_all": false }),
        json!({ "scopes": ["@acme"] }),
        json!({ "scopes": [] }),
        json!({ "orgs": ["acme"] }),
        json!({ "orgs_permission": "read-only" }),
        json!({ "orgs_permission": "no-access" }),
        json!({ "unexpected_restriction": true }),
    ] {
        assert_bad_request(&payload);
    }

    let null_optionals = parse_json(&json!({
        "name": null,
        "cidr_whitelist": null,
        "packages": null,
        "scopes": null,
        "orgs": null,
        "orgs_permission": null,
        "expires": null,
        "expires_in": null,
    }))
    .unwrap();
    assert_eq!(null_optionals.access, TokenAccess::ReadWrite);
    assert_eq!(null_optionals.expiry, TokenExpiry::Never);
}

#[test]
fn interprets_expires_as_days_or_rfc3339_and_expires_in_as_seconds() {
    let seven_days = parse_json(&json!({ "expires": 7 })).unwrap();
    assert_eq!(seven_days.expiry, TokenExpiry::AfterMillis(7 * MILLIS_PER_DAY));
    assert_eq!(seven_days.expiry.resolve(FIXED_NOW).unwrap(), Some(FIXED_NOW + 7 * MILLIS_PER_DAY));

    let string_days = parse_json(&json!({ "expires": "30" })).unwrap();
    assert_eq!(string_days.expiry, TokenExpiry::AfterMillis(30 * MILLIS_PER_DAY));

    let rfc3339 = parse_json(&json!({ "expires": "2030-01-01T00:00:00Z" })).unwrap();
    assert_eq!(rfc3339.expiry, TokenExpiry::AtTimestamp(1_893_456_000_000));

    let max_rfc3339 = parse_json(&json!({ "expires": "9999-12-31T23:59:59.999Z" })).unwrap();
    assert_eq!(max_rfc3339.expiry, TokenExpiry::AtTimestamp(MAX_TOKEN_TIMESTAMP_MILLIS));

    let seconds = parse_json(&json!({ "expires_in": 120 })).unwrap();
    assert_eq!(seconds.expiry, TokenExpiry::AfterMillis(120 * MILLIS_PER_SECOND));
    assert_eq!(
        seconds.expiry.resolve(FIXED_NOW).unwrap(),
        Some(FIXED_NOW + 120 * MILLIS_PER_SECOND),
    );
}

#[test]
fn rejects_invalid_past_conflicting_and_overflowing_expiries() {
    for payload in [
        json!({ "expires": 0 }),
        json!({ "expires": "0" }),
        json!({ "expires": -7 }),
        json!({ "expires": 1.5 }),
        json!({ "expires": "+7" }),
        json!({ "expires": true }),
        json!({ "expires": "not-a-date" }),
        json!({ "expires": "2020-01-01T00:00:00Z" }),
        json!({ "expires": 3_000_000 }),
        json!({ "expires": "99999999999999999999999999" }),
        json!({ "expires_in": 0 }),
        json!({ "expires_in": u64::MAX }),
        json!({ "expires": 7, "expires_in": 60 }),
    ] {
        assert_bad_request(&payload);
    }
}

fn within(
    request: &Value,
    cidr_whitelist: &[&str],
    expires_at: Option<u64>,
) -> Result<pnpr_auth::NewToken, RegistryError> {
    let limits = CredentialLimits {
        cidr_whitelist: cidr_whitelist
            .iter()
            .map(ToString::to_string)
            .collect(),
        expires_at,
    };
    parse_json(request)?.within(limits)?.into_new_token(FIXED_NOW)
}

#[test]
fn a_token_inherits_the_callers_cidr_whitelist() {
    let token = within(&json!({}), &["10.1.0.0/16"], None).unwrap();
    assert_eq!(token.cidr_whitelist, ["10.1.0.0/16"]);
    let token =
        within(&json!({ "cidr_whitelist": ["10.1.2.0/24"] }), &["10.1.0.0/16"], None).unwrap();
    assert_eq!(token.cidr_whitelist, ["10.1.2.0/24"]);
    let token = within(&json!({ "cidr_whitelist": ["10.1.2.0/24"] }), &[], None).unwrap();
    assert_eq!(token.cidr_whitelist, ["10.1.2.0/24"]);
}

#[test]
fn a_token_cannot_widen_the_callers_cidr_whitelist() {
    for wider in [json!(["10.0.0.0/8"]), json!(["10.1.2.0/24", "192.168.0.1"])] {
        let request = json!({ "cidr_whitelist": wider });
        assert!(
            matches!(
                within(&request, &["10.1.0.0/16"], None),
                Err(RegistryError::BadRequest { .. }),
            ),
            "{request}",
        );
    }
}

#[test]
fn a_token_expires_no_later_than_the_caller() {
    let latest = FIXED_NOW + 3600 * MILLIS_PER_SECOND;
    assert_eq!(within(&json!({}), &[], Some(latest)).unwrap().expires_at, Some(latest));
    assert_eq!(
        within(&json!({ "expires": 1 }), &[], Some(latest)).unwrap().expires_at,
        Some(latest),
    );
    assert_eq!(
        within(&json!({ "expires_in": 60 }), &[], Some(latest)).unwrap().expires_at,
        Some(FIXED_NOW + 60 * MILLIS_PER_SECOND),
    );
    assert_eq!(within(&json!({}), &[], None).unwrap().expires_at, None);
}
