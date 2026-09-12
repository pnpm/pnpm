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
        }}"#,
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

/// A registry may abbreviate either marker to a bare `1`
/// ([pnpm/pnpm#14432](https://github.com/pnpm/pnpm/issues/14432)); the
/// marker still counts, and the manifest still decodes.
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

/// Tolerating unrecognized marker shapes must not promote the shapes a
/// registry uses to say the marker is *unset* into trust evidence. The
/// TypeScript resolver reads these fields for truthiness, so a version
/// carrying `false`, `0`, or `""` has no evidence in either stack and
/// stays subject to the `no-downgrade` rejection.
#[test]
fn a_falsy_marker_grants_no_evidence() {
    for marker in ["false", "0", "0.0", "-0", r#""""#] {
        let version = parse(
            &format!(r#"{{ "trustedPublisher": {marker}, "approver": {marker} }}"#),
            &format!(r#"{{ "provenance": {marker} }}"#),
        );
        let npm_user = version.npm_user.as_ref();
        assert!(
            npm_user.is_none_or(|user| user.trusted_publisher.is_none()),
            "trustedPublisher of {marker} is not a trusted publisher",
        );
        assert!(
            npm_user.is_none_or(|user| user.approver.is_none()),
            "approver of {marker} is not an approver",
        );
        assert!(
            version.dist.attestations.as_ref().is_none_or(|att| att.provenance.is_none()),
            "provenance of {marker} is not a provenance attestation",
        );
    }
}

/// The marker's own `null` — as opposed to a missing or null container
/// around it — is the shape that reaches the decoder, and it means the
/// registry named the field to say it holds nothing.
#[test]
fn a_null_marker_stays_absent() {
    let version =
        parse(r#"{ "trustedPublisher": null, "approver": null }"#, r#"{ "provenance": null }"#);

    let npm_user = version.npm_user.as_ref();
    assert!(
        npm_user.is_none_or(|user| user.trusted_publisher.is_none()),
        "a null trustedPublisher is not a trusted publisher",
    );
    assert!(
        npm_user.is_none_or(|user| user.approver.is_none()),
        "a null approver is not an approver",
    );
    assert!(
        version.dist.attestations.as_ref().is_none_or(|att| att.provenance.is_none()),
        "a null provenance is not a provenance attestation",
    );
}

#[test]
fn an_absent_or_null_container_leaves_every_marker_absent() {
    for container in ["null", "{}"] {
        let version = parse(container, container);
        assert!(
            version.npm_user.as_ref().is_none_or(|user| user.trusted_publisher.is_none()),
            "trustedPublisher should be absent for _npmUser {container}",
        );
        assert!(
            version.dist.attestations.as_ref().is_none_or(|att| att.provenance.is_none()),
            "provenance should be absent for attestations {container}",
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
        }}"#,
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

/// A count too large for a `usize`, or a float the parser has already
/// rounded, is not a count. It has to read as absent rather than clamp
/// or round, since a saturating cast would hand the extractor
/// `usize::MAX` as though the registry had reported it.
#[test]
fn an_out_of_range_advisory_count_does_not_clamp() {
    for encoded in [
        "1e100",
        "1e30",
        "18446744073709551616",
        "1.8446744073709552e19",
        "9007199254740993.0",
        "9007199254740992.0",
    ] {
        let version = parse_with(&format!(r#", "unpackedSize": {encoded}"#), "");
        assert_eq!(version.dist.unpacked_size, None, "unpackedSize {encoded} is out of range");
        let version = parse_with(&format!(r#", "fileCount": {encoded}"#), "");
        assert_eq!(version.dist.file_count, None, "fileCount {encoded} is out of range");
    }

    let version = parse_with(r#", "unpackedSize": 1099511627776.0"#, "");
    assert_eq!(
        version.dist.unpacked_size,
        usize::try_from(1_099_511_627_776_u64).ok(),
        "a terabyte-scale float is still in range",
    );

    let version = parse_with(&format!(r#", "unpackedSize": {}"#, usize::MAX), "");
    assert_eq!(
        version.dist.unpacked_size,
        Some(usize::MAX),
        "an exactly encoded usize::MAX is still in range",
    );
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

/// The publisher's display fields are decoration; the markers beside
/// them decide the version's trust rank. Decoding the record as a unit
/// means a mistyped `name` would otherwise discard a real `approver`,
/// weakening the rank and letting `no-downgrade` reject a version the
/// TypeScript resolver accepts.
#[test]
fn a_mistyped_publisher_name_keeps_the_trust_markers() {
    let version = parse_with(
        "",
        r#", "_npmUser": { "name": 1, "email": [], "approver": {}, "trustedPublisher": { "id": "github" } }"#,
    );

    let user = version.npm_user.as_ref().expect("_npmUser survives a mistyped name");
    assert_eq!(user.name, None, "a non-string name reads as absent");
    assert_eq!(user.email, None, "a non-string email reads as absent");
    assert!(user.approver.is_some(), "the approver marker survives");
    assert_eq!(
        user.trusted_publisher.as_ref().and_then(|publisher| publisher.id.as_deref()),
        Some("github"),
        "the trusted publisher survives with its body intact",
    );
}

/// `dist.attestations` is a container like `_npmUser`: only the
/// `provenance` marker inside it is read, so neither the container's own
/// shape nor its `url` sibling may cost the version.
#[test]
fn an_off_shape_attestations_container_or_url_keeps_the_version() {
    for encoded in ["1", r#""signed""#, "[]", "true"] {
        let version = parse_with(&format!(r#", "attestations": {encoded}"#), "");
        assert!(version.dist.attestations.is_none(), "attestations {encoded} carries no metadata");
    }

    let version = parse_with(r#", "attestations": { "provenance": {}, "url": 7 }"#, "");
    let attestations = version.dist.attestations.as_ref().expect("an object attestations decodes");
    assert_eq!(attestations.url, None, "a non-string url reads as absent");
    assert!(attestations.provenance.is_some(), "the provenance marker survives a mistyped url");
}

/// A `peerDependenciesMeta` entry names a peer even when its value is not
/// an object: the TypeScript resolver keeps the name and finds no
/// `optional === true` inside it.
#[test]
fn an_off_shape_peer_meta_entry_keeps_its_name_with_optional_unset() {
    for encoded in ["true", "1", r#""optional""#, "[]", "null"] {
        let version = parse_with(
            "",
            &format!(
                r#", "peerDependenciesMeta": {{ "react": {encoded}, "vue": {{ "optional": true }} }}"#,
            ),
        );
        let meta = version.peer_dependencies_meta.as_ref().expect("the map decodes");
        assert_eq!(meta.get("react").map(|entry| entry.optional), Some(None), "react: {encoded}");
        assert_eq!(
            meta.get("vue").map(|entry| entry.optional),
            Some(Some(true)),
            "vue keeps its flag beside react: {encoded}",
        );
    }

    for encoded in ["1", "[]", r#""react""#] {
        let version = parse_with("", &format!(r#", "peerDependenciesMeta": {encoded}"#));
        assert!(
            version.peer_dependencies_meta.is_none(),
            "peerDependenciesMeta {encoded} has no entries",
        );
    }
}
