use super::{name, offered, target, version};
use crate::{Packages, WheelMetadata, locked_solution, validate_locked};
use pep508_rs::Requirement;

#[test]
fn locked_solutions_reject_sources_declared_only_by_inactive_requirements() {
    let (packages, source) = source_packages();
    let requirements: Vec<Requirement> =
        ["demo".to_string(), format!("demo @ {source} ; sys_platform == 'never'")]
            .into_iter()
            .map(|requirement| requirement.parse().unwrap())
            .collect();
    let error = locked_solution(&packages, &requirements, &target().environment).unwrap_err();
    assert!(error.to_string().contains("inactive source"), "{error}");
    let error = validate_locked(&packages, &requirements, &target().environment).unwrap_err();
    assert!(error.to_string().contains("inactive source"), "{error}");
}

#[test]
fn locked_solutions_accept_sources_declared_by_active_requirements() {
    let (packages, source) = source_packages();
    let requirements: Vec<Requirement> =
        ["demo".to_string(), format!("demo @ {source} ; sys_platform == 'linux'")]
            .into_iter()
            .map(|requirement| requirement.parse().unwrap())
            .collect();
    validate_locked(&packages, &requirements, &target().environment).unwrap();
}

fn source_packages() -> (Packages, String) {
    let mut packages = offered(&target(), &[("demo", &["demo-1.0.0-py3-none-any.whl"])]);
    let source = packages.candidates[&name("demo")][&version("1.0.0")]
        .wheel()
        .unwrap()
        .url
        .clone();
    packages.direct_urls.insert(name("demo"), source.clone());
    packages.metadata.insert(
        (name("demo"), version("1.0.0")),
        WheelMetadata::parse("Name: demo\nVersion: 1.0.0\n").unwrap(),
    );
    (packages, source)
}
