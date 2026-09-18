use super::{
    AddRequest, EcosystemPackageSpecifier, PackageSpecifierPlan, RegistryPackageSpecifier,
};

/// The selectors a plan leaves for the npm add path, which is what every
/// assertion on them reads.
fn node_selectors(plan: &PackageSpecifierPlan) -> Vec<&str> {
    plan.node_packages
        .iter()
        .map(AddRequest::selector)
        .collect()
}

#[test]
fn partitions_node_and_cargo_specifiers() {
    let plan = PackageSpecifierPlan::parse([
        "lodash@4".into(),
        "crate:serde".into(),
        "crate:tokio@~1.43".into(),
    ])
    .unwrap();

    assert_eq!(node_selectors(&plan), ["lodash@4"]);
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
fn a_node_selector_keeps_the_allocation_the_command_line_gave_it() {
    let request = AddRequest::from("lodash@4");
    let allocation = request.selector().as_ptr();

    let plan = PackageSpecifierPlan::parse([request]).unwrap();

    assert_eq!(plan.node_packages[0].selector().as_ptr(), allocation);
}

#[test]
fn rejects_invalid_cargo_specifiers_before_manifest_initialization() {
    for specifier in
        ["crate:", "crate:serde@", "crate:bad/name", "crate:serde@workspace:*", "crate:serde@^"]
    {
        assert!(
            PackageSpecifierPlan::parse([specifier.into()]).is_err(),
            "{specifier} must be rejected",
        );
    }
}

#[test]
fn partitions_python_requirements_without_applying_node_or_cargo_semver() {
    let plan = PackageSpecifierPlan::parse([
        "npm-package@1".into(),
        "crate:serde@1".into(),
        "pypi:Some_Package[fast]@~=1.2".into(),
        "pypi:other@2.0rc1".into(),
    ])
    .unwrap();
    assert_eq!(node_selectors(&plan), ["npm-package@1"]);
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
        assert!(PackageSpecifierPlan::parse([specifier.into()]).is_err(), "{specifier}");
    }
}

#[test]
fn routes_purls_to_the_ecosystem_named_by_their_type() {
    let plan = PackageSpecifierPlan::parse([
        "pkg:npm/express@4.18.2".into(),
        "pkg:cargo/serde@1.0.188".into(),
        "pkg:pypi/requests@2.31.0".into(),
    ])
    .unwrap();

    assert_eq!(node_selectors(&plan), ["express@4.18.2"]);
    assert_eq!(
        plan.ecosystem_packages,
        [
            EcosystemPackageSpecifier::Cargo(RegistryPackageSpecifier {
                name: "serde".to_string(),
                version_spec: Some("=1.0.188".to_string()),
            }),
            EcosystemPackageSpecifier::Python("requests==2.31.0".to_string()),
        ],
    );
}

#[test]
fn a_versionless_purl_leaves_the_version_to_the_resolver() {
    let plan = PackageSpecifierPlan::parse([
        "pkg:npm/express".into(),
        "pkg:cargo/serde".into(),
        "pkg:pypi/requests".into(),
    ])
    .unwrap();

    assert_eq!(node_selectors(&plan), ["express"]);
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
    let plan = PackageSpecifierPlan::parse([
        "pkg:npm/%40babel/core@7.22.0".into(),
        "pkg:npm/@babel/traverse".into(),
        "pkg:npm/babel/types@7.22.0".into(),
    ])
    .unwrap();

    assert_eq!(
        node_selectors(&plan),
        ["@babel/core@7.22.0", "@babel/traverse", "@babel/types@7.22.0"],
    );
}

#[test]
fn a_pypi_purl_name_is_normalized_like_any_other_python_requirement() {
    let plan = PackageSpecifierPlan::parse(["pkg:pypi/Some_Package@1.2".into()]).unwrap();

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
        ("pkg:cargo/serde@%5e", "pkg:cargo/serde@%5e does not carry a valid Cargo version"),
        ("pkg:cargo/serde@%5E1.0", "pkg:cargo/serde@%5E1.0 does not carry a valid Cargo version"),
        ("pkg:cargo/serde@*", "pkg:cargo/serde@* does not carry a valid Cargo version"),
        ("pkg:cargo/serde@1.0", "pkg:cargo/serde@1.0 does not carry a valid Cargo version"),
    ] {
        let message_received =
            PackageSpecifierPlan::parse([specifier.into()]).expect_err(specifier).to_string();
        assert_eq!(message_received, message, "{specifier}");
    }
}

/// A purl names a package in a registry, so the request it becomes says so,
/// and the add path installs it rather than reading it as the package
/// manager or the runtime that shares its name. The mark belongs to the
/// request rather than to its text, so the two spellings of one selector
/// keep their own meanings in one command.
#[test]
fn a_purl_marks_the_request_it_becomes_as_a_package_to_install() {
    let plan = PackageSpecifierPlan::parse([
        "node@22.0.0".into(),
        "pkg:npm/node@22.0.0".into(),
        "lodash@4".into(),
    ])
    .unwrap();

    assert_eq!(node_selectors(&plan), ["node@22.0.0", "node@22.0.0", "lodash@4"]);
    let may_name_a_tool: Vec<bool> = plan.node_packages
        .iter()
        .map(AddRequest::may_name_a_tool)
        .collect();
    assert_eq!(may_name_a_tool, [true, false, true]);
}

/// Each ecosystem's selector has a grammar a decoded component could reach
/// into: `name@spec` is an npm alias, and a PEP 508 requirement carries
/// extras and markers. Smuggling one in has to be rejected, not resolved.
#[test]
fn rejects_a_purl_whose_components_would_rewrite_the_selector() {
    for (specifier, message) in [
        ("pkg:npm/%2e%2e%2fescape", "pkg:npm/%2e%2e%2fescape has an invalid purl name"),
        (
            "pkg:npm/express%40npm%3Aevil",
            "pkg:npm/express%40npm%3Aevil does not name a valid npm package",
        ),
        // The decoded version spells `npm:evil@1.0.0`, a colon paired with
        // an at-sign, which is how a URL spells credentials. The message
        // stops at that colon rather than quote what follows it.
        (
            "pkg:npm/express@npm%3Aevil%401.0.0",
            "pkg:npm/express@npm does not carry a valid npm version",
        ),
        ("pkg:cargo/foo%401.0.0", "invalid Cargo package name in pkg:cargo/foo%401.0.0"),
        (
            "pkg:npm/%40babel%2Fcore@7.22.0",
            "pkg:npm/%40babel%2Fcore@7.22.0 has an invalid purl name",
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
            PackageSpecifierPlan::parse([specifier.into()]).expect_err(specifier).to_string();
        assert_eq!(message_received, message, "{specifier}");
    }
}

/// A selector reaches a diagnostic from the command line, so a credential in
/// a qualifier and a control character anywhere have to be stripped on the
/// way out rather than printed back.
#[test]
fn a_rejected_selector_is_redacted_and_sanitized_before_it_is_printed() {
    let message = |specifier: &str| {
        PackageSpecifierPlan::parse([specifier.into()]).expect_err(specifier).to_string()
    };

    // Percent-encoding hides an authority from a check made on the raw
    // text, and encoding the `?` or `#` moves the value out of the
    // qualifier the message drops, into the name, version, or type. What
    // decoding leaves unreadable to that check, in turn, is anything put
    // between the scheme and its slashes: a space, a byte that is not
    // valid UTF-8, a malformed escape, a doubly encoded slash.
    for credentials in [
        message("pkg:npm/foo?repository_url=https://user:pass@example.test"),
        message("pkg:npm/foo?repository_url=https:%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo?repository_url=https://user:pass%40example.test"),
        message("pkg:npm/foo#https:%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo%3Frepository_url=https:%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo%23https:%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo@1.0.0%3Fx=https:%2F%2Fuser:pass%40example.test"),
        message("pkg:BAD%3Fhttps:%2F%2Fuser:pass%40example.test/foo"),
        message("crate:foo@https:%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo@https:%FF%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo@https:%20%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo@https:%ZZ%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo@https:%1F%20%2F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo@https:%2F%252F%2Fuser:pass%40example.test"),
        message("pkg:npm/foo@https%3A%2F%2Fuser%3Apass%40example.test"),
        message("crate:foo@https:%20%2F%2Fuser:pass%40example.test"),
    ] {
        assert!(!credentials.contains("pass"), "{credentials}");
    }

    // A space inside the userinfo hides it from the scan too, where it
    // leaves a password that reads as two innocent words.
    assert_eq!(
        message("pkg:npm/foo@https:%2F%2Fuser:pa%20ss%40example.test"),
        "pkg:npm/foo@https does not carry a valid npm version",
    );

    // A selector with nothing to redact is still quoted as it was written,
    // so an encoded component is visible as the reason it was rejected.
    let encoded = message("pkg:npm/%40babel%2Fcore@7.22.0");
    assert!(encoded.contains("%40babel%2Fcore"), "{encoded}");

    let control = message("pkg:maven/foo\u{1b}[31m/bar@1");
    assert!(!control.contains('\u{1b}'), "{control:?}");

    // The rejected type is named alongside the selector, so it is a second
    // place the raw text reaches a message.
    let control_type = message("pkg:bad\u{1b}[31m/lodash");
    assert!(!control_type.contains('\u{1b}'), "{control_type:?}");
}
