use super::{EcosystemPackageSpecifier, PackageSpecifierPlan, RegistryPackageSpecifier};

#[test]
fn partitions_node_and_cargo_specifiers() {
    let plan = PackageSpecifierPlan::parse(&[
        "lodash@4".to_string(),
        "crate:serde".to_string(),
        "crate:tokio@~1.43".to_string(),
    ])
    .unwrap();

    assert_eq!(plan.node_packages, ["lodash@4"]);
    assert_eq!(
        plan.ecosystem_packages,
        [
            EcosystemPackageSpecifier::Cargo(RegistryPackageSpecifier {
                name: "serde".to_string(),
                version_spec: None,
            }),
            EcosystemPackageSpecifier::Cargo(RegistryPackageSpecifier {
                name: "tokio".to_string(),
                version_spec: Some("~1.43".to_string()),
            }),
        ],
    );
}

#[test]
fn rejects_invalid_cargo_specifiers_before_manifest_initialization() {
    for specifier in ["crate:", "crate:serde@", "crate:bad/name", "crate:serde@workspace:"] {
        assert!(
            PackageSpecifierPlan::parse(&[specifier.to_string()]).is_err(),
            "{specifier} must be rejected",
        );
    }
}
