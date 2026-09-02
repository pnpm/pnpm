use crate::PackageVersion;

/// A per-version manifest with `attestations` / `_npmUser` bodies spliced in.
fn manifest_json(npm_user: &str, attestations: &str) -> String {
    format!(
        r#"{{
            "name": "acme",
            "version": "1.0.0",
            "_npmUser": {npm_user},
            "dist": {{
                "tarball": "https://registry/acme-1.0.0.tgz",
                "attestations": {attestations}
            }}
        }}"#
    )
}

fn parse(npm_user: &str, attestations: &str) -> PackageVersion {
    serde_json::from_str(&manifest_json(npm_user, attestations)).expect("deserialize manifest")
}

#[test]
fn an_object_marker_keeps_its_body() {
    let version = parse(
        r#"{ "trustedPublisher": { "id": "github", "oidcConfigId": "release" } }"#,
        r#"{ "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } }"#,
    );

    let publisher = version
        .npm_user
        .as_ref()
        .and_then(|user| user.trusted_publisher.as_ref())
        .expect("trustedPublisher present");
    assert_eq!(publisher.id.as_deref(), Some("github"));
    assert_eq!(publisher.oidc_config_id.as_deref(), Some("release"));

    let provenance = version
        .dist
        .attestations
        .as_ref()
        .and_then(|att| att.provenance.as_ref())
        .expect("provenance present");
    assert_eq!(provenance.predicate_type.as_deref(), Some("https://slsa.dev/provenance/v1"));
}

/// A registry may abbreviate either marker to a bare `1`. Decoding that
/// strictly failed the whole manifest, which then read as a version the
/// packument never listed, leaving resolution with "no version found for
/// the latest tag"
/// ([pnpm/pnpm#14432](https://github.com/pnpm/pnpm/issues/14432)).
#[test]
fn a_numeric_marker_still_counts_as_present() {
    let version = parse(r#"{ "trustedPublisher": 1 }"#, r#"{ "provenance": 1 }"#);

    let publisher = version
        .npm_user
        .as_ref()
        .and_then(|user| user.trusted_publisher.as_ref())
        .expect("trustedPublisher present");
    assert_eq!(publisher.id, None);
    assert_eq!(publisher.oidc_config_id, None);

    let provenance = version
        .dist
        .attestations
        .as_ref()
        .and_then(|att| att.provenance.as_ref())
        .expect("provenance present");
    assert_eq!(provenance.predicate_type, None);
}

#[test]
fn a_marker_body_of_any_other_shape_still_counts_as_present() {
    for marker in [r#""yes""#, "true", "[]", "{}", r#"{ "predicateType": 7 }"#] {
        let version = parse(
            &format!(r#"{{ "trustedPublisher": {marker} }}"#),
            &format!(r#"{{ "provenance": {marker} }}"#),
        );
        assert!(
            version.npm_user.as_ref().is_some_and(|user| user.trusted_publisher.is_some()),
            "trustedPublisher marked with {marker} should count as present",
        );
        assert!(
            version.dist.attestations.as_ref().is_some_and(|att| att.provenance.is_some()),
            "provenance marked with {marker} should count as present",
        );
    }
}

#[test]
fn an_absent_or_null_marker_stays_absent() {
    for marker in ["null", "{}"] {
        let version = parse(marker, marker);
        assert!(
            version.npm_user.as_ref().is_none_or(|user| user.trusted_publisher.is_none()),
            "trustedPublisher should be absent for _npmUser {marker}",
        );
        assert!(
            version.dist.attestations.as_ref().is_none_or(|att| att.provenance.is_none()),
            "provenance should be absent for attestations {marker}",
        );
    }
}

/// The `approver` marker takes the same tolerance: it is the sibling
/// presence-only field on `_npmUser`, and a strict decode of it would
/// erase the version exactly the same way.
#[test]
fn the_approver_marker_is_equally_tolerant() {
    let version = parse(r#"{ "approver": 1 }"#, "{}");
    assert!(version.npm_user.as_ref().is_some_and(|user| user.approver.is_some()));
}

/// A manifest with arbitrary fragments spliced into `dist` and the top level.
fn parse_with(dist_extra: &str, top_extra: &str) -> PackageVersion {
    let json = format!(
        r#"{{
            "name": "acme",
            "version": "1.0.0"
            {top_extra},
            "dist": {{
                "tarball": "https://registry/acme-1.0.0.tgz"
                {dist_extra}
            }}
        }}"#
    );
    serde_json::from_str(&json).expect("deserialize manifest")
}

#[test]
fn an_advisory_count_accepts_every_numeric_encoding() {
    for (encoded, expected) in [
        ("12345", Some(12345)),
        ("12345.0", Some(12345)),
        (r#""12345""#, Some(12345)),
        (r#"" 12345 ""#, Some(12345)),
        ("0", Some(0)),
    ] {
        let version = parse_with(&format!(r#", "unpackedSize": {encoded}"#), "");
        assert_eq!(version.dist.unpacked_size, expected, "unpackedSize {encoded}");
        let version = parse_with(&format!(r#", "fileCount": {encoded}"#), "");
        assert_eq!(version.dist.file_count, expected, "fileCount {encoded}");
    }
}

/// A count pnpm can't make sense of degrades to "not reported" — the
/// same state an abbreviated packument leaves it in — rather than
/// costing the version. Only the extractor's allocation hint reads it.
#[test]
fn an_unusable_advisory_count_degrades_to_absent() {
    for encoded in ["-1", "12.5", r#""not a number""#, "true", "null", "{}", "[]"] {
        let version = parse_with(&format!(r#", "unpackedSize": {encoded}"#), "");
        assert_eq!(version.dist.unpacked_size, None, "unpackedSize {encoded}");
    }
    let version = parse_with("", "");
    assert_eq!(version.dist.unpacked_size, None, "an omitted unpackedSize is absent");
}

/// The TypeScript resolver tests this flag with `=== true`, so only a
/// real boolean may produce `Some(true)` here.
#[test]
fn a_peer_optional_flag_counts_only_when_it_is_a_real_boolean() {
    let peer_meta = |encoded: &str| {
        parse_with(
            "",
            &format!(r#", "peerDependenciesMeta": {{ "react": {{ "optional": {encoded} }} }}"#),
        )
        .peer_dependencies_meta
        .as_ref()
        .and_then(|map| map.get("react"))
        .expect("react entry present")
        .optional
    };

    assert_eq!(peer_meta("true"), Some(true));
    assert_eq!(peer_meta("false"), Some(false));
    for encoded in [r#""true""#, "1", "null", "{}"] {
        assert_eq!(peer_meta(encoded), None, "optional {encoded} is not a boolean `true`");
    }
}

/// `_npmUser` is a container, not a marker: everything pnpm reads lives
/// inside it, so a registry sending a scalar in its place has supplied
/// no publisher metadata — and must not thereby erase the version.
#[test]
fn a_non_object_npm_user_decodes_as_absent() {
    for encoded in ["1", r#""alice <alice@example.com>""#, "[]", "true"] {
        let version = parse_with("", &format!(r#", "_npmUser": {encoded}"#));
        assert!(version.npm_user.is_none(), "_npmUser {encoded} carries no metadata");
    }

    let version = parse_with("", r#", "_npmUser": { "name": "alice", "approver": {} }"#);
    let user = version.npm_user.as_ref().expect("an object _npmUser still decodes");
    assert_eq!(user.name.as_deref(), Some("alice"));
    assert!(user.approver.is_some());
}
