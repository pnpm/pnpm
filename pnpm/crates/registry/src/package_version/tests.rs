use super::{AuthHeaders, PackageTag, PackageVersion, ThrottledClient};

#[tokio::test]
async fn fetch_from_registry_attaches_authorization_header() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{
        "name": "acme",
        "version": "1.0.0",
        "dist": {
            "integrity": "sha512-AAAA",
            "shasum": "0000000000000000000000000000000000000000",
            "tarball": "https://registry.test/acme-1.0.0.tgz"
        }
    }"#;
    let mock = server
        .mock("GET", "/acme/latest")
        .match_header("authorization", "Bearer top-secret")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::from_creds_map([(
        pnpm_network::nerf_dart(&registry),
        "Bearer top-secret".to_owned(),
    )]);

    let pkg_version = PackageVersion::fetch_from_registry(
        "acme",
        PackageTag::Latest,
        &client,
        &registry,
        &auth_headers,
    )
    .await
    .expect("server should accept the request once the bearer header is attached");
    assert_eq!(pkg_version.name, "acme");
    mock.assert_async().await;
}

/// Dropping either field would silently treat optional peers as
/// required (auto-installed via `autoInstallPeers`) and skip
/// `optionalDependencies` entirely.
#[test]
fn deserializes_optional_dependencies_and_peer_dependencies_meta() {
    let body = r#"{
        "name": "unstorage",
        "version": "1.17.5",
        "dist": {
            "integrity": "sha512-AAAA",
            "shasum": "0000000000000000000000000000000000000000",
            "tarball": "https://registry.test/unstorage-1.17.5.tgz"
        },
        "peerDependencies": {
            "@vercel/kv": "^1 || ^2 || ^3",
            "ioredis": "^5.4.2"
        },
        "peerDependenciesMeta": {
            "@vercel/kv": { "optional": true },
            "ioredis": { "optional": true }
        },
        "optionalDependencies": {
            "sharp": "^0.34.0"
        }
    }"#;

    let pkg: PackageVersion =
        serde_json::from_str(body).expect("deserialize PackageVersion fixture");

    let optional = pkg.optional_dependencies.as_ref().expect("optionalDependencies present");
    assert_eq!(optional.get("sharp").map(String::as_str), Some("^0.34.0"));

    let peer_meta = pkg.peer_dependencies_meta.as_ref().expect("peerDependenciesMeta present");
    assert_eq!(peer_meta["@vercel/kv"].optional, Some(true));
    assert_eq!(peer_meta["ioredis"].optional, Some(true));

    // The JSON shape `serde_json::to_value(pkg)` produces feeds
    // `extract_children` / `extract_peer_dependencies` downstream;
    // both consume the camelCase keys verbatim.
    let value = serde_json::to_value(&pkg).expect("serialize PackageVersion");
    assert!(value.get("optionalDependencies").is_some_and(serde_json::Value::is_object));
    assert!(value.get("peerDependenciesMeta").is_some_and(serde_json::Value::is_object));
}

/// A minimal decodable manifest with fragments spliced into `dist`,
/// `_npmUser`, and the top level, for the wire-shape tolerance tests.
fn manifest_with(dist_extra: &str, npm_user: &str, top_extra: &str) -> String {
    format!(
        r#"{{
            "name": "acme",
            "version": "1.0.0"
            {top_extra},
            "_npmUser": {npm_user},
            "dist": {{
                "tarball": "https://registry/acme-1.0.0.tgz",
                "integrity": "sha512-AAAA"
                {dist_extra}
            }}
        }}"#,
    )
}

fn decodes(json: &str) -> bool {
    serde_json::from_str::<PackageVersion>(json).is_ok()
}

/// The registry wire format grows fields and reshapes the trust markers
/// without warning, and a manifest that fails to decode is skipped as if
/// the version were never published — so a shape pnpm doesn't model can
/// silently erase `dist-tags.latest` from a packument. Every shape here
/// must keep costing nothing.
#[test]
fn net_new_fields_and_marker_reshapes_never_cost_the_version() {
    let cases = [
        (
            "provenance gains net-new fields",
            manifest_with(
                r#", "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1", "sigstoreBundle": { "x": 1 } } }"#,
                "{}",
                "",
            ),
        ),
        (
            "predicateType reshapes from string to object",
            manifest_with(
                r#", "attestations": { "provenance": { "predicateType": { "uri": "https://slsa.dev/provenance/v2" } } }"#,
                "{}",
                "",
            ),
        ),
        (
            "provenance reshapes from object to a list of attestations",
            manifest_with(
                r#", "attestations": { "provenance": [ { "predicateType": "a" } ] }"#,
                "{}",
                "",
            ),
        ),
        (
            "provenance abbreviates to a presence marker",
            manifest_with(r#", "attestations": { "provenance": 1 }"#, "{}", ""),
        ),
        (
            "attestations gains a sibling of provenance",
            manifest_with(
                r#", "attestations": { "provenance": {}, "publishAttestation": { "kind": "x" } }"#,
                "{}",
                "",
            ),
        ),
        (
            "trustedPublisher gains net-new fields",
            manifest_with(
                "",
                r#"{ "trustedPublisher": { "id": "github", "oidcConfigId": "r", "workflowRef": "w" } }"#,
                "",
            ),
        ),
        (
            "trustedPublisher abbreviates to a presence marker",
            manifest_with("", r#"{ "trustedPublisher": 1 }"#, ""),
        ),
        (
            "dist gains a net-new field",
            manifest_with(r#", "signatures": [ { "sig": "s" } ]"#, "{}", ""),
        ),
        (
            "the manifest gains a net-new top-level field",
            manifest_with("", "{}", r#", "hasShrinkwrap": false"#),
        ),
        ("unpackedSize as a float", manifest_with(r#", "unpackedSize": 12345.0"#, "{}", "")),
        ("unpackedSize as a string", manifest_with(r#", "unpackedSize": "12345""#, "{}", "")),
        ("fileCount as a float", manifest_with(r#", "fileCount": 12.0"#, "{}", "")),
        (
            "peerDependenciesMeta.optional as a string",
            manifest_with(
                "",
                "{}",
                r#", "peerDependenciesMeta": { "react": { "optional": "true" } }"#,
            ),
        ),
        ("_npmUser abbreviated to a presence marker", manifest_with("", "1", "")),
        ("_npmUser as a maintainer string", manifest_with("", r#""alice""#, "")),
    ];

    for (label, json) in cases {
        assert!(decodes(&json), "{label} must not make the version undecodable");
    }
}

/// Counterpart to
/// [`net_new_fields_and_marker_reshapes_never_cost_the_version`]: what a
/// version manifest still has to get right. Each of these fails the
/// manifest, and so the version, by design — pnpm cannot install a
/// version it can't name, locate, or verify, and a quieter failure would
/// only defer the problem. Everything else about a manifest degrades to
/// `None` instead.
///
/// A case that starts decoding here is a behavior change, not a fix:
/// weigh what pnpm would do with the version afterwards before removing
/// it from this list.
#[test]
fn a_version_pnpm_could_not_install_still_fails_the_manifest() {
    let cases = [
        // Without a verifiable tarball hash the version cannot be locked;
        // the snapshot builder rejects it either way, and failing here
        // keeps pnpm from silently resolving an older version instead.
        (
            "integrity with an algorithm ssri doesn't know",
            manifest_with("", "{}", "").replace("sha512-AAAA", "blake3-AAAA"),
        ),
        (
            "integrity that isn't a subresource integrity string",
            manifest_with("", "{}", "").replace("sha512-AAAA", "not-an-integrity"),
        ),
        // No tarball to fetch, and no version to order against its peers.
        ("dist omitted entirely", r#"{ "name": "acme", "version": "1.0.0" }"#.to_string()),
        (
            "a version that isn't semver",
            manifest_with("", "{}", "").replace(r#""version": "1.0.0""#, r#""version": "1.0.0.0""#),
        ),
    ];

    for (label, json) in cases {
        assert!(!decodes(&json), "{label} must still fail the manifest");
    }
}
