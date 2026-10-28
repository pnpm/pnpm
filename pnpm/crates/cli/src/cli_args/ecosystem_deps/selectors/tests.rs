use super::{EcosystemFilterKind, patterns_for_ecosystem, validate_ecosystem_selectors};
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
