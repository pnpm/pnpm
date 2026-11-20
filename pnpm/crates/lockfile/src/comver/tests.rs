use super::ComVer;
use pretty_assertions::assert_eq;

#[test]
fn parse() {
    assert_eq!("6.0".parse::<ComVer>().unwrap(), ComVer::new(6, 0));
}

#[test]
fn to_string() {
    assert_eq!(ComVer::new(6, 0).to_string(), "6.0");
}
