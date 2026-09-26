use super::Inputs;
use pep508_rs::Requirement;

#[test]
fn lockfile_resolution_settings_invalidate_replay() {
    let requirements = vec!["demo".parse::<Requirement>().unwrap()];
    let mut previous = Inputs::declared(&requirements, &[], &[], "https://example.test/simple/");
    let mut wanted = Inputs::declared(&requirements, &[], &[], "https://example.test/simple/");
    wanted.set_resolution_settings(&["https://extra.test/simple/".to_string()], &[], &[]);
    assert_eq!(previous.differs_from(&wanted), Some("the Python index changed"));
    previous.set_resolution_settings(&["https://extra.test/simple/".to_string()], &[], &[]);
    wanted.set_resolution_settings(
        &["https://extra.test/simple/".to_string()],
        &["demo==2".parse().unwrap()],
        &[],
    );
    assert_eq!(previous.differs_from(&wanted), Some("the Python overrides or constraints changed"));
    wanted.set_resolution_settings(
        &["https://extra.test/simple/".to_string()],
        &[],
        &["demo<2".parse().unwrap()],
    );
    assert_eq!(previous.differs_from(&wanted), Some("the Python overrides or constraints changed"));
}

#[test]
fn the_members_sharing_an_environment_invalidate_replay() {
    let requirements = vec!["demo".parse::<Requirement>().unwrap()];
    let previous = Inputs::declared(&requirements, &[], &[], "https://example.test/simple/");
    let mut wanted = Inputs::declared(&requirements, &[], &[], "https://example.test/simple/");
    assert_eq!(previous.differs_from(&wanted), None);
    wanted.set_members(vec!["packages/a".to_string()]);
    assert_eq!(
        previous.differs_from(&wanted),
        Some("the projects sharing the Python environment changed"),
    );
}

#[test]
fn changing_package_routes_invalidates_lockfile_replay() {
    let requirements = vec!["alpha".parse::<Requirement>().unwrap()];
    let mut previous = Inputs::declared(&requirements, &[], &[], "https://public.test/simple/");
    let mut wanted = Inputs::declared(&requirements, &[], &[], "https://public.test/simple/");
    previous.set_registry_packages(std::collections::BTreeMap::from([(
        "https://private.test/simple/".to_string(),
        vec!["alpha".to_string()],
    )]));
    wanted.set_registry_packages(std::collections::BTreeMap::from([(
        "https://private.test/simple/".to_string(),
        vec!["beta".to_string()],
    )]));
    assert_eq!(previous.differs_from(&wanted), Some("the Python index changed"));
    let serialized = serde_json::to_string(&previous).unwrap();
    let round_trip: Inputs = serde_json::from_str(&serialized).unwrap();
    assert_eq!(previous, round_trip);
}
