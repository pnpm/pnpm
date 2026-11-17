use super::{contains, parse};
use semver::Version;
use std::collections::BTreeMap;

fn strings(versions: &[&str]) -> Vec<String> {
    versions
        .iter()
        .map(|version| (*version).to_string())
        .collect()
}

#[test]
fn parse_sorts_newest_first_and_drops_duplicates() {
    let parsed = parse(&BTreeMap::from([(
        "foo".to_string(),
        strings(&["1.0.0", "2.0.0", "1.0.0", "1.5.0"]),
    )]))
    .unwrap();

    assert_eq!(
        parsed["foo"],
        [Version::new(2, 0, 0), Version::new(1, 5, 0), Version::new(1, 0, 0)],
    );
    assert!(contains(&parsed, "foo", &Version::new(1, 5, 0)));
    assert!(!contains(&parsed, "foo", &Version::new(1, 6, 0)));
    assert!(!contains(&parsed, "bar", &Version::new(1, 0, 0)));
}
