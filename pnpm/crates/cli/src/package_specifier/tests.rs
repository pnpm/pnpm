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

#[test]
fn routes_purls_to_the_ecosystem_named_by_their_type() {
    let plan = PackageSpecifierPlan::parse(&[
        "pkg:npm/express@4.18.2".into(),
        "pkg:cargo/serde@1.0.188".into(),
        "pkg:pypi/requests@2.31.0".into(),
    ])
    .unwrap();

    assert_eq!(plan.node_packages, ["express@4.18.2"]);
    assert_eq!(
        plan.ecosystem_packages,
        [
            EcosystemPackageSpecifier::Cargo(RegistryPackageSpecifier {
                name: "serde".to_string(),
                version_spec: Some("1.0.188".to_string()),
            }),
            EcosystemPackageSpecifier::Python("requests==2.31.0".to_string()),
        ],
    );
}

#[test]
fn a_versionless_purl_leaves_the_version_to_the_resolver() {
    let plan = PackageSpecifierPlan::parse(&[
        "pkg:npm/express".into(),
        "pkg:cargo/serde".into(),
        "pkg:pypi/requests".into(),
    ])
    .unwrap();

    assert_eq!(plan.node_packages, ["express"]);
    assert_eq!(
        plan.ecosystem_packages,
        [
            EcosystemPackageSpecifier::Cargo(RegistryPackageSpecifier {
                name: "serde".to_string(),
                version_spec: None,
            }),
            EcosystemPackageSpecifier::Python("requests".to_string()),
        ],
    );
}

#[test]
fn a_purl_namespace_becomes_an_npm_scope() {
    let plan = PackageSpecifierPlan::parse(&[
        "pkg:npm/%40babel/core@7.22.0".into(),
        "pkg:npm/@babel/traverse".into(),
        "pkg:npm/babel/types@7.22.0".into(),
    ])
    .unwrap();

    assert_eq!(
        plan.node_packages,
        ["@babel/core@7.22.0", "@babel/traverse", "@babel/types@7.22.0"],
    );
}

#[test]
fn a_pypi_purl_name_is_normalized_like_any_other_python_requirement() {
    let plan = PackageSpecifierPlan::parse(&["pkg:pypi/Some_Package@1.2".into()]).unwrap();

    assert_eq!(
        plan.ecosystem_packages,
        [EcosystemPackageSpecifier::Python("some-package==1.2".to_string())],
    );
}

#[test]
fn rejects_purls_pnpm_cannot_add() {
    for (specifier, message) in [
        (
            "pkg:maven/org.apache.commons/io@1.3.4",
            "pkg:maven/org.apache.commons/io@1.3.4 has purl type `maven`, but pnpm can add only `npm`, `cargo`, and `pypi` packages",
        ),
        (
            "pkg:cargo/rust-lang/serde@1.0.188",
            "pkg:cargo/rust-lang/serde@1.0.188 has a purl namespace, which type `cargo` does not define",
        ),
        (
            "pkg:pypi/psf/requests@2.31.0",
            "pkg:pypi/psf/requests@2.31.0 has a purl namespace, which type `pypi` does not define",
        ),
        (
            "pkg:npm/%40babel/types/core",
            "pkg:npm/%40babel/types/core has a multi-segment purl namespace, but an npm scope is a single segment",
        ),
        ("pkg:cargo/serde@%5e", "invalid Cargo version requirement in pkg:cargo/serde@%5e"),
    ] {
        let message_received =
            PackageSpecifierPlan::parse(&[specifier.into()]).expect_err(specifier).to_string();
        assert_eq!(message_received, message, "{specifier}");
    }
}

/// Each ecosystem's selector has a grammar a decoded component could reach
/// into: `name@spec` is an npm alias, and a PEP 508 requirement carries
/// extras and markers. Smuggling one in has to be rejected, not resolved.
#[test]
fn rejects_a_purl_whose_components_would_rewrite_the_selector() {
    for (specifier, message) in [
        ("pkg:npm/%2e%2e%2fescape", "pkg:npm/%2e%2e%2fescape does not name a valid npm package"),
        (
            "pkg:npm/express%40npm%3Aevil",
            "pkg:npm/express%40npm%3Aevil does not name a valid npm package",
        ),
        (
            "pkg:npm/express@npm%3Aevil%401.0.0",
            "pkg:npm/express@npm%3Aevil%401.0.0 does not carry a valid npm version",
        ),
        (
            "pkg:pypi/requests%5Bsecurity%5D@2.31.0",
            "pkg:pypi/requests%5Bsecurity%5D@2.31.0 does not name a valid PyPI project",
        ),
        (
            "pkg:pypi/requests@2.31.0%20%3B%20os_name%3D%3D%22nt%22",
            "pkg:pypi/requests@2.31.0%20%3B%20os_name%3D%3D%22nt%22 does not carry a valid PyPI version",
        ),
    ] {
        let message_received =
            PackageSpecifierPlan::parse(&[specifier.into()]).expect_err(specifier).to_string();
        assert_eq!(message_received, message, "{specifier}");
    }
}
