use super::{contains, parse, retain_indexed};
use crate::registry::{CRATES_IO_SOURCE, Registry};
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

#[test]
fn retain_indexed_drops_versions_and_crates_the_index_lacks() {
    let index = concat!(
        r#"{"name":"foo","vers":"1.0.0","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}"#,
        "\n",
        r#"{"name":"foo","vers":"1.1.0","deps":[],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":true}"#,
    );
    let registry =
        Registry::new(&BTreeMap::from([("foo".to_string(), index.to_string())]), CRATES_IO_SOURCE)
            .unwrap();
    let mut preferred = parse(&BTreeMap::from([
        ("foo".to_string(), strings(&["1.1.0", "1.0.0", "9.9.9"])),
        ("bar".to_string(), strings(&["1.0.0"])),
    ]))
    .unwrap();

    retain_indexed(&mut preferred, &registry);

    assert_eq!(
        preferred,
        BTreeMap::from([("foo".to_string(), vec![Version::new(1, 1, 0), Version::new(1, 0, 0)],)]),
    );
}
