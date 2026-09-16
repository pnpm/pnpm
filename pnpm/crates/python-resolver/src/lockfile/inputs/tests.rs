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
