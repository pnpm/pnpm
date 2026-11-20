use super::{
    EcosystemFilterKind, parse_update_selectors, patterns_for_ecosystem,
    validate_ecosystem_selectors,
};
use pnpm_config::Config;

#[test]
fn patterns_for_ecosystem_routes_prefixed_and_bare_selectors() {
    let selectors = vec![
        "crate:serde".to_string(),
        "pypi:requests".to_string(),
        "pkg:cargo/tokio@1.0.0".to_string(),
        "pkg:pypi/httpx@0.28.0".to_string(),
        "pkg:npm/chalk@5.0.0".to_string(),
        "foo*".to_string(),
    ];

    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Cargo).unwrap(),
        vec!["serde", "tokio", "foo*"],
    );
    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Python).unwrap(),
        vec!["requests", "httpx", "foo*"],
    );
    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Node).unwrap(),
        vec!["chalk", "foo*"],
    );
}

#[test]
fn validate_ecosystem_selectors_rejects_disabled_ecosystems() {
    let config = Config::default();
    assert!(validate_ecosystem_selectors(&["crate:serde".to_string()], &config).is_err());
    assert!(validate_ecosystem_selectors(&["pypi:requests".to_string()], &config).is_err());
}

#[test]
fn patterns_for_ecosystem_routes_negated_prefixed_selectors() {
    let selectors = vec!["!crate:sha2".to_string(), "!pypi:requests".to_string()];

    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Cargo).unwrap(),
        vec!["!sha2"],
    );
    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Python).unwrap(),
        vec!["!requests"],
    );
}

#[test]
fn patterns_for_ecosystem_parses_negated_pypi_requirements() {
    let selectors = vec!["!pypi:requests>=2".to_string()];

    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Python).unwrap(),
        vec!["!requests"],
    );
}

#[test]
fn patterns_for_ecosystem_preserves_negated_pypi_wildcards_and_at_versions() {
    let selectors = vec!["!pypi:requests*".to_string(), "!pypi:httpx@2".to_string()];

    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Python).unwrap(),
        vec!["!requests*", "!httpx"],
    );
}

#[test]
fn patterns_for_ecosystem_parses_pypi_version_wildcards_as_requirements() {
    let selectors = vec!["!pypi:requests==2.*".to_string()];

    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Python).unwrap(),
        vec!["!requests"],
    );
}

#[test]
fn patterns_for_ecosystem_normalizes_pypi_name_wildcards() {
    let selectors = vec!["!pypi:Zope_Interface*".to_string()];

    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Python).unwrap(),
        vec!["!zope-interface*"],
    );
}

#[test]
fn update_selectors_keep_negated_cargo_exclusions_out_of_node_selectors() {
    let mut config = Config::default();
    config.cargo.enabled = true;
    let parsed = parse_update_selectors(&["!crate:sha2".to_string()], &config).unwrap();

    assert!(parsed.node_selectors.is_empty());
    assert!(parsed.cargo_requests.is_empty());
    assert!(parsed.bare_patterns.is_empty());
    assert_eq!(parsed.cargo_exclusions, vec!["sha2"]);
}

#[test]
fn update_selectors_route_negated_ecosystem_purls_to_their_exclusions() {
    let mut config = Config::default();
    config.cargo.enabled = true;
    config.python.enabled = true;
    let parsed = parse_update_selectors(
        &["!pkg:cargo/serde@1.0.0".to_string(), "!pkg:pypi/requests@2.0.0".to_string()],
        &config,
    )
    .unwrap();

    assert!(parsed.node_selectors.is_empty());
    assert!(parsed.cargo_requests.is_empty());
    assert!(parsed.python_requests.is_empty());
    assert!(parsed.bare_patterns.is_empty());
    assert_eq!(parsed.cargo_exclusions, vec!["serde"]);
    assert_eq!(parsed.python_exclusions, vec!["requests"]);
}

#[test]
fn update_selectors_keep_prefixed_exclusions_scoped_alongside_positive_requests() {
    let mut config = Config::default();
    config.cargo.enabled = true;
    config.python.enabled = true;
    let parsed = parse_update_selectors(
        &[
            "crate:serde".to_string(),
            "!crate:serde".to_string(),
            "pypi:requests".to_string(),
            "!pypi:requests".to_string(),
            "foo*".to_string(),
        ],
        &config,
    )
    .unwrap();

    assert_eq!(parsed.cargo_requests.len(), 1);
    assert_eq!(parsed.cargo_exclusions, vec!["serde"]);
    assert_eq!(parsed.python_requests.len(), 1);
    assert_eq!(parsed.python_exclusions, vec!["requests"]);
    assert_eq!(parsed.bare_patterns, vec!["foo*"]);
}

#[test]
fn update_selectors_reject_direct_url_pypi_requirements() {
    let mut config = Config::default();
    config.python.enabled = true;
    let err = parse_update_selectors(
        &["pypi:six @ https://example.org/six-1.16.0-py2.py3-none-any.whl".to_string()],
        &config,
    )
    .expect_err("direct URL requirement should be rejected by update")
    .to_string();
    assert!(err.contains("direct URL requirements are not supported by pnpm update"), "{err}");
}

#[test]
fn update_selectors_drop_orphan_exclusions_across_ecosystems() {
    let mut config = Config::default();
    config.cargo.enabled = true;
    config.python.enabled = true;

    let cargo_and_exclude =
        parse_update_selectors(&["crate:serde".to_string(), "!lodash".to_string()], &config)
            .unwrap();
    assert!(cargo_and_exclude.node_selectors.is_empty());
    assert!(cargo_and_exclude.bare_patterns.is_empty());
    assert_eq!(cargo_and_exclude.cargo_requests.len(), 1);
    assert_eq!(cargo_and_exclude.cargo_exclusions, vec!["lodash"]);

    let bare_and_exclude =
        parse_update_selectors(&["react".to_string(), "!lodash".to_string()], &config).unwrap();
    assert_eq!(bare_and_exclude.node_selectors, vec!["react", "!lodash"]);
    assert!(bare_and_exclude.bare_patterns.is_empty());
    assert_eq!(bare_and_exclude.cargo_exclusions, vec!["lodash"]);
    assert_eq!(bare_and_exclude.python_exclusions, vec!["lodash"]);

    let scoped_and_exclude =
        parse_update_selectors(&["@scope/pkg".to_string(), "!lodash".to_string()], &config)
            .unwrap();
    assert_eq!(scoped_and_exclude.node_selectors, vec!["@scope/pkg", "!lodash"]);
    assert!(scoped_and_exclude.bare_patterns.is_empty());
    assert!(scoped_and_exclude.cargo_requests.is_empty());
    assert!(scoped_and_exclude.python_requests.is_empty());

    let only_exclude = parse_update_selectors(&["!lodash".to_string()], &config).unwrap();
    assert_eq!(only_exclude.node_selectors, vec!["!lodash"]);
    assert_eq!(only_exclude.bare_patterns, vec!["!lodash"]);
}

#[test]
fn patterns_for_ecosystem_drops_orphan_exclusions_when_another_ecosystem_is_selected() {
    let selectors = vec!["crate:serde".to_string(), "!lodash".to_string()];

    assert_eq!(
        patterns_for_ecosystem(&selectors, EcosystemFilterKind::Cargo).unwrap(),
        vec!["serde", "!lodash"],
    );
    assert!(patterns_for_ecosystem(&selectors, EcosystemFilterKind::Python).unwrap().is_empty());
    assert!(patterns_for_ecosystem(&selectors, EcosystemFilterKind::Node).unwrap().is_empty());
}
