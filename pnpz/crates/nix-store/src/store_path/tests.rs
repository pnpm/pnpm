use super::StorePath;
use pretty_assertions::assert_eq;

const HELLO: &str = "/nix/store/1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1";

#[test]
fn parses_an_absolute_path() {
    let path = StorePath::from_absolute(HELLO).unwrap();
    assert_eq!(path.hash_part(), "1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl");
    assert_eq!(path.name(), "hello-2.12.1");
    assert_eq!(path.to_string(), HELLO);
}

#[test]
fn parses_a_base_name() {
    let path = StorePath::from_base_name("1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1").unwrap();
    assert_eq!(path, StorePath::from_absolute(HELLO).unwrap());
}

#[test]
fn rejects_a_path_outside_the_store() {
    StorePath::from_absolute("/usr/store/1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello").unwrap_err();
}

#[test]
fn rejects_a_path_below_a_store_path() {
    StorePath::from_absolute(&format!("{HELLO}/bin/hello")).unwrap_err();
}

#[test]
fn rejects_a_name_that_escapes_the_store() {
    StorePath::from_base_name("1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-../../etc").unwrap_err();
}

#[test]
fn rejects_a_hash_with_a_digit_outside_the_alphabet() {
    StorePath::from_base_name("eq8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello").unwrap_err();
}

#[test]
fn rejects_a_missing_name() {
    StorePath::from_base_name("1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-").unwrap_err();
    StorePath::from_base_name("1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl").unwrap_err();
}
