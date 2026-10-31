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
