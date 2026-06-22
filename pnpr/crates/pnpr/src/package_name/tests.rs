use super::PackageName;

#[test]
fn accepts_unscoped() {
    let name = PackageName::parse("lodash").unwrap();
    assert_eq!(name.as_str(), "lodash");
    assert_eq!(name.tarball_name_for_version("4.17.21"), "lodash-4.17.21.tgz");
    name.parse_tarball_name("lodash-4.17.21.tgz").unwrap();
}

#[test]
fn accepts_scoped() {
    let name = PackageName::parse("@types/node").unwrap();
    assert_eq!(name.as_str(), "@types/node");
    assert_eq!(name.tarball_name_for_version("20.0.0"), "node-20.0.0.tgz");
    name.parse_tarball_name("node-20.0.0.tgz").unwrap();
}

#[test]
fn rejects_traversal() {
    assert!(PackageName::parse("..").is_err());
    assert!(PackageName::parse("foo/../bar").is_err());
    assert!(PackageName::parse("@scope/..").is_err());
}

#[test]
fn rejects_dot_prefix() {
    assert!(PackageName::parse(".hidden").is_err());
}

#[test]
fn rejects_tarball_for_other_package() {
    let name = PackageName::parse("foo").unwrap();
    assert!(name.parse_tarball_name("bar-1.0.0.tgz").is_err());
    assert!(name.parse_tarball_name("../foo-1.0.0.tgz").is_err());
    assert!(name.parse_tarball_name("foo-1.0.0").is_err());
}
