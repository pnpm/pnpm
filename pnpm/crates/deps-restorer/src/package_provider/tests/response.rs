use super::super::{
    ProviderRequestBundle, ProviderResponse, parse_provider_response, validate_provider_response,
};
use super::helpers::{Fixture, tarball_metadata};
use pnpm_lockfile::SnapshotEntry;
use pretty_assertions::assert_eq;

fn two_node_bundle() -> ProviderRequestBundle {
    Fixture::new()
        .with("foo@1.0.0", SnapshotEntry::default(), tarball_metadata())
        .with(
            "bar@2.0.0",
            SnapshotEntry { optional: true, ..SnapshotEntry::default() },
            tarball_metadata(),
        )
        .build()
        .expect("build request")
        .expect("non-empty request")
}

fn response(paths: &[(&str, &str)], skipped: &[&str]) -> ProviderResponse {
    ProviderResponse {
        protocol: Some(1),
        paths: Some(
            paths
                .iter()
                .map(|(dep_path, dir)| (dep_path.to_string(), dir.to_string()))
                .collect(),
        ),
        skipped: Some(
            skipped
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
    }
}

#[test]
fn response_must_be_valid_json() {
    let error = parse_provider_response("/provider", b"not json")
        .expect_err("invalid JSON must be rejected");
    assert_eq!(
        error.to_string(),
        r#"The package provider at "/provider" did not return valid JSON"#
    );
}

#[test]
fn response_protocol_must_match() {
    let bad_protocol = br#"{"protocol": 2, "paths": {}}"#;
    let error = parse_provider_response("/provider", bad_protocol)
        .expect_err("protocol mismatch must be rejected");
    assert_eq!(
        error.to_string(),
        r#"The package provider at "/provider" returned an unsupported response (protocol 2)"#,
    );

    let no_protocol = br#"{"paths": {}}"#;
    let error = parse_provider_response("/provider", no_protocol)
        .expect_err("missing protocol must be rejected");
    assert_eq!(
        error.to_string(),
        r#"The package provider at "/provider" returned an unsupported response (protocol missing)"#,
    );
}

#[test]
fn skipping_a_non_optional_package_is_rejected() {
    let bundle = two_node_bundle();
    let error = validate_provider_response(
        &bundle,
        response(&[("bar@2.0.0", "/nix/store/bar")], &["foo@1.0.0"]),
    )
    .expect_err("skipping required node must be rejected");
    assert_eq!(
        error.to_string(),
        "The package provider skipped foo@1.0.0, which is not an optional dependency",
    );
}

#[test]
fn missing_path_for_an_installed_node_is_rejected() {
    let bundle = two_node_bundle();
    let error =
        validate_provider_response(&bundle, response(&[("foo@1.0.0", "/nix/store/foo")], &[]))
            .expect_err("missing node must be rejected");
    assert_eq!(error.to_string(), "The package provider returned no path for bar@2.0.0");
}

#[test]
fn relative_or_empty_path_is_rejected() {
    let bundle = two_node_bundle();
    let error = validate_provider_response(
        &bundle,
        response(&[("foo@1.0.0", "relative/path"), ("bar@2.0.0", "/nix/store/bar")], &[]),
    )
    .expect_err("relative path must be rejected");
    assert_eq!(
        error.to_string(),
        "The package provider returned a relative path for foo@1.0.0: relative/path",
    );

    let error = validate_provider_response(
        &bundle,
        response(&[("foo@1.0.0", ""), ("bar@2.0.0", "/nix/store/bar")], &[]),
    )
    .expect_err("empty path must be rejected");
    assert_eq!(error.to_string(), "The package provider returned a relative path for foo@1.0.0: ",);
}

#[test]
fn valid_response_extracts_paths_and_skipped_keys() {
    let bundle = two_node_bundle();
    let output = validate_provider_response(
        &bundle,
        response(&[("foo@1.0.0", "/nix/store/foo")], &["bar@2.0.0"]),
    )
    .expect("valid response must validate");

    assert_eq!(output.skipped.len(), 1);
    assert_eq!(output.skipped[0].to_string(), "bar@2.0.0");
    assert_eq!(output.paths.len(), 1);
    let foo_key = bundle.key_by_dep_path["foo@1.0.0"].clone();
    assert_eq!(output.paths[&foo_key], std::path::PathBuf::from("/nix/store/foo"));
}
