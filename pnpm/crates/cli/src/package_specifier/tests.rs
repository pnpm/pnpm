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
    for specifier in
        ["crate:", "crate:serde@", "crate:bad/name", "crate:serde@workspace:*", "crate:serde@^"]
    {
        assert!(
            PackageSpecifierPlan::parse(&[specifier.to_string()]).is_err(),
            "{specifier} must be rejected",
        );
    }
}

#[test]
fn partitions_python_requirements_without_applying_node_or_cargo_semver() {
    let plan = PackageSpecifierPlan::parse(&[
        "npm-package@1".into(),
        "crate:serde@1".into(),
        "pypi:Some_Package[fast]@~=1.2".into(),
        "pypi:other@2.0rc1".into(),
    ])
    .unwrap();
    assert_eq!(plan.node_packages, ["npm-package@1"]);
    assert!(plan.has_cargo());
    assert!(plan.has_python());
    assert_eq!(
        plan.ecosystem_packages[1],
        EcosystemPackageSpecifier::Python("some-package[fast]~=1.2".into()),
    );
    assert_eq!(
        plan.ecosystem_packages[2],
        EcosystemPackageSpecifier::Python("other==2.0rc1".into()),
    );
    for specifier in
        ["pypi:", "pypi:alpha@", "pypi:alpha@^1.0", "pypi:alpha@https://example.org/a.whl"]
    {
        assert!(PackageSpecifierPlan::parse(&[specifier.into()]).is_err(), "{specifier}");
    }
}
