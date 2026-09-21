use super::{LatestPicker, ResolveLatestError};
use crate::resolution_policy::PickPolicy;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_resolving_npm_resolver::shared_packument_fetch_locker;
use std::sync::Arc;
use tempfile::tempdir;

/// A packument that lists `latest` and serves it an unreadable manifest.
/// `dist.integrity` decodes strictly — a tarball hash pnpm cannot read
/// is one it cannot verify — so the version is listed yet unhydratable,
/// which is exactly the state a dangling tag is indistinguishable from.
fn undecodable_latest_packument(latest: &str) -> String {
    format!(
        r#"{{
            "name": "acme",
            "dist-tags": {{ "latest": "{latest}" }},
            "time": {{ "{latest}": "2020-01-10T08:30:00.000Z" }},
            "versions": {{
                "{latest}": {{
                    "name": "acme",
                    "version": "{latest}",
                    "dist": {{
                        "tarball": "https://registry/acme.tgz",
                        "integrity": "not-an-integrity"
                    }}
                }}
            }}
        }}"#,
    )
}

async fn resolve_latest_error(latest: &str) -> ResolveLatestError {
    let dir = tempdir().expect("tempdir");
    let mut server = mockito::Server::new_async().await;
    let _packument = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(undecodable_latest_packument(latest))
        .create_async()
        .await;

    let mut config = Config::new();
    config.cache_dir = dir.path().join("cache");
    config.registry = format!("{}/", server.url());
    // A maturity cutoff routes the resolve through the package picker
    // rather than the dist-tag endpoint, which is the only path that can
    // tell an undecodable manifest from an empty tag.
    config.minimum_release_age = Some(24 * 60);

    let http_client = ThrottledClient::default();
    let policy = PickPolicy::from_config(&config).expect("derive pick policy");
    let picker = LatestPicker::new(
        &config,
        &http_client,
        policy,
        Arc::default(),
        shared_packument_fetch_locker(),
    );

    picker.resolve("acme", true).await.expect_err("latest cannot resolve")
}

#[tokio::test]
async fn an_undecodable_latest_manifest_is_reported_instead_of_an_empty_tag() {
    let error = resolve_latest_error("1.0.0").await;

    let ResolveLatestError::UndecodableLatestManifest { name, version, error } = error else {
        panic!("expected an undecodable-manifest error, got: {error}");
    };
    assert_eq!(name, "acme");
    assert_eq!(version, "1.0.0");
    assert!(error.contains("integrity"), "the error names the field pnpm choked on: {error}");
}

/// `dist-tags.latest` and the decoder's quoting of the value it rejected
/// are both the registry's text. Rendering either verbatim would let a
/// packument redraw the diagnostic with escape sequences or split it
/// across lines.
#[tokio::test]
async fn registry_text_reaches_the_diagnostic_stripped_of_control_characters() {
    let error = resolve_latest_error(r"1.0.0\u001b[31m\nnot the error").await;

    let ResolveLatestError::UndecodableLatestManifest { version, .. } = &error else {
        panic!("expected an undecodable-manifest error, got: {error}");
    };
    assert_eq!(
        version, "1.0.0[31mnot the error",
        "the version keeps its text and loses the control characters",
    );

    let rendered = error.to_string();
    assert!(
        !rendered.chars().any(char::is_control),
        "no control character survives into the diagnostic: {rendered:?}",
    );
}

use pnpm_registry::PackageVersion;
use serde_json::json;

use super::{exact_pins, pin_is_exempt};

/// Built by deserialization, the way a packument entry reaches the picker,
/// so the fixture does not have to name every field of `PackageVersion`.
fn candidate(
    dependencies: &serde_json::Value,
    optional_dependencies: &serde_json::Value,
) -> PackageVersion {
    serde_json::from_value(json!({
        "name": "parent",
        "version": "1.0.0",
        "dist": { "tarball": "https://registry.npmjs.org/parent/-/parent-1.0.0.tgz", "shasum": "" },
        "dependencies": dependencies,
        "optionalDependencies": optional_dependencies,
    }))
    .expect("valid package version")
}

fn pins(candidate: &PackageVersion) -> Vec<(String, String)> {
    let mut pins: Vec<(String, String)> =
        exact_pins(candidate, &std::collections::HashSet::from(["gh".to_string()]))
            .map(|(spec, _)| (spec.name, spec.fetch_spec))
            .collect();
    pins.sort();
    pins
}

#[test]
fn reports_exact_pins_from_both_dependency_groups() {
    // Optional dependencies count: the lockfile records every platform's
    // binary, so one too young blocks the install on every platform.
    let candidate = candidate(&json!({ "oxc": "0.146.0" }), &json!({ "binding-darwin": "1.2.5" }));

    assert_eq!(
        pins(&candidate),
        vec![
            ("binding-darwin".to_string(), "1.2.5".to_string()),
            ("oxc".to_string(), "0.146.0".to_string()),
        ],
    );
}

#[test]
fn passes_over_dependencies_a_range_can_satisfy() {
    // A range has other versions to fall back on, and choosing among them is
    // the install's resolution, not this pre-check's.
    let candidate = candidate(
        &json!({ "caret": "^1.0.0", "tilde": "~2.3.4", "any": "*", "exact": "3.0.0" }),
        &json!({}),
    );

    assert_eq!(pins(&candidate), vec![("exact".to_string(), "3.0.0".to_string())]);
}

#[test]
fn passes_over_specifiers_that_name_no_registry_version() {
    // `=1.0.0` is a range that happens to admit one version, and the rest
    // resolve through other protocols entirely. None can be looked up by
    // version in a packument, so none is judged here.
    let candidate = candidate(
        &json!({
            "equals": "=1.0.0",
            "tag": "latest",
            "workspace": "workspace:*",
            "git": "github:owner/repo",
            "external": "https://external.example/child/-/child-1.0.0.tgz",
        }),
        &json!({}),
    );

    assert_eq!(pins(&candidate), Vec::<(String, String)>::new());
}

#[test]
fn reports_nothing_for_a_candidate_that_declares_no_dependencies() {
    assert_eq!(pins(&candidate(&json!({}), &json!({}))), Vec::<(String, String)>::new());
}

#[test]
fn an_optional_declaration_overrides_a_same_name_dependency() {
    // npm resolves a name declared in both groups to its optional entry, so
    // the `dependencies` specifier never reaches the install and must not
    // decide whether the candidate is passed over.
    let candidate = candidate(
        &json!({ "shared": "1.0.0" }),
        &json!({ "shared": "^2.0.0", "binding": "1.2.5" }),
    );

    assert_eq!(pins(&candidate), vec![("binding".to_string(), "1.2.5".to_string())]);
}

#[test]
fn an_optional_override_that_is_itself_a_pin_is_the_one_judged() {
    let candidate = candidate(&json!({ "shared": "1.0.0" }), &json!({ "shared": "2.0.0" }));

    assert_eq!(pins(&candidate), vec![("shared".to_string(), "2.0.0".to_string())]);
}

fn exclude(patterns: &[&str]) -> pnpm_config::version_policy::PackageVersionPolicy {
    pnpm_config::version_policy::create_package_version_policy(patterns)
        .expect("valid exclude patterns")
}

#[test]
fn a_name_only_exclusion_exempts_every_version_of_that_package() {
    let policy = exclude(&["child"]);

    assert!(pin_is_exempt(Some(&policy), "child", "1.0.0"));
    assert!(pin_is_exempt(Some(&policy), "child", "2.0.0"));
}

#[test]
fn a_version_qualified_exclusion_exempts_only_the_versions_it_names() {
    // Excluding child@1.0.0 says nothing about child@2.0.0, so a candidate
    // pinning the latter is still judged on its age.
    let policy = exclude(&["child@1.0.0"]);

    assert!(pin_is_exempt(Some(&policy), "child", "1.0.0"));
    assert!(!pin_is_exempt(Some(&policy), "child", "2.0.0"));
}

#[test]
fn a_package_the_policy_does_not_name_is_never_exempt() {
    let policy = exclude(&["other"]);

    assert!(!pin_is_exempt(Some(&policy), "child", "1.0.0"));
    assert!(!pin_is_exempt(None, "child", "1.0.0"));
}

#[tokio::test]
async fn latest_retains_the_first_candidate_when_all_exact_pins_are_immature() {
    let dir = tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let parent = json!({
        "name": "parent",
        "dist-tags": { "latest": "2.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00Z", "2.0.0": "2020-01-01T00:00:00Z" },
        "versions": {
            "1.0.0": { "name": "parent", "version": "1.0.0", "dist": { "tarball": "https://registry/parent.tgz" }, "dependencies": { "child": "1.0.0" } },
            "2.0.0": { "name": "parent", "version": "2.0.0", "dist": { "tarball": "https://registry/parent.tgz" }, "dependencies": { "child": "1.0.0" } }
        }
    });
    let child = json!({
        "name": "child",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2099-01-01T00:00:00Z" },
        "versions": { "1.0.0": { "name": "child", "version": "1.0.0", "dist": { "tarball": "https://registry/child.tgz" } } }
    });
    let _parent = server
        .mock("GET", "/parent")
        .with_status(200)
        .with_body(parent.to_string())
        .create_async()
        .await;
    let _child = server
        .mock("GET", "/child")
        .with_status(200)
        .with_body(child.to_string())
        .create_async()
        .await;
    let mut config = Config::new();
    config.cache_dir = dir.path().join("cache");
    config.registry = format!("{}/", server.url());
    config.minimum_release_age = Some(1440);
    let http_client = ThrottledClient::default();
    let picker = LatestPicker::new(
        &config,
        &http_client,
        PickPolicy::from_config(&config).unwrap(),
        Arc::default(),
        shared_packument_fetch_locker(),
    );
    assert_eq!(
        picker
            .resolve("parent", true)
            .await
            .unwrap()
            .version
            .to_string(),
        "2.0.0",
    );
}

#[test]
fn normalizes_exact_pins_and_parses_named_registries() {
    let candidate = candidate(
        &json!({ "one": "v1.2.3", "two": "V1.2.3", "alias": "gh:child@2.0.0" }),
        &json!({ "optional": "gh:3.0.0" }),
    );
    assert_eq!(
        pins(&candidate),
        vec![
            ("child".to_string(), "2.0.0".to_string()),
            ("one".to_string(), "1.2.3".to_string()),
            ("optional".to_string(), "3.0.0".to_string()),
            ("two".to_string(), "1.2.3".to_string()),
        ],
    );
}

#[tokio::test]
async fn external_tarball_pins_do_not_query_registry_publication_times() {
    let dir = tempdir().unwrap();
    let mut registry = mockito::Server::new_async().await;
    let child = registry
        .mock("GET", "/child")
        .with_status(200)
        .with_body(
            json!({
                "name": "child", "dist-tags": { "latest": "1.0.0" },
                "time": { "1.0.0": "2099-01-01T00:00:00Z" },
                "versions": { "1.0.0": {
                    "name": "child", "version": "1.0.0",
                    "dist": { "tarball": "https://registry.example/child-1.0.0.tgz" },
                } },
            })
            .to_string(),
        )
        .expect(0)
        .create_async()
        .await;
    let mut config = Config::new();
    config.cache_dir = dir.path().join("cache");
    config.registry = format!("{}/", registry.url());
    config.minimum_release_age = Some(1440);
    let client = ThrottledClient::default();
    let picker = LatestPicker::new(
        &config,
        &client,
        PickPolicy::from_config(&config).unwrap(),
        Arc::default(),
        shared_packument_fetch_locker(),
    );
    let parent = candidate(
        &json!({ "child": "https://external.example/child/-/child-1.0.0.tgz" }),
        &json!({}),
    );
    assert!(picker.pins_only_installable_versions(&parent, true).await.unwrap());
    child.assert_async().await;
}
