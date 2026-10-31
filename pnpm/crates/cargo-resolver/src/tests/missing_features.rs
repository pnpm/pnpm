use crate::missing_features;
use semver::Version;

const HASH_INDEX: &str = r#"{"name":"hash","vers":"0.10.0","deps":[{"name":"backend","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"asm":[],"std":[]},"yanked":false}
{"name":"hash","vers":"0.11.0","deps":[{"name":"backend","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{"std":[]},"yanked":false}
{"name":"hash","vers":"0.12.0","deps":[],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{"std":[]},"yanked":false}"#;

fn missing(version: &str, features: &[&str]) -> Vec<String> {
    let features: Vec<String> = features
        .iter()
        .map(ToString::to_string)
        .collect();
    missing_features("hash", HASH_INDEX, &Version::parse(version).unwrap(), &features).unwrap()
}

#[test]
fn reports_features_a_version_does_not_define() {
    assert_eq!(missing("0.10.0", &["asm", "std"]), Vec::<String>::new());
    assert_eq!(missing("0.11.0", &["asm", "std"]), ["asm"]);
}

#[test]
fn implicit_optional_dependency_features_and_dependency_features_count_as_defined() {
    assert_eq!(
        missing("0.11.0", &["backend", "backend/extra", "backend?/extra"]),
        Vec::<String>::new(),
    );
}

#[test]
fn a_dependency_feature_is_missing_once_the_dependency_is_dropped() {
    assert_eq!(
        missing("0.12.0", &["backend/extra", "backend?/extra"]),
        ["backend/extra", "backend?/extra"],
    );
}

#[test]
fn a_version_missing_from_the_index_is_an_error() {
    let error =
        missing_features("hash", HASH_INDEX, &Version::parse("0.13.0").unwrap(), &[]).unwrap_err();
    assert!(error.to_string().contains("no version 0.13.0"), "{error}");
}
