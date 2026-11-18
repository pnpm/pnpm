use super::{is_git_hosted_dep_path, parse_dep_path_name_version, short_package_label};

#[test]
fn labels_registry_and_source_dependencies() {
    assert_eq!(short_package_label("foo@1.0.0"), "foo");
    assert_eq!(short_package_label("foo@link:../foo"), "foo (local)");
    assert_eq!(short_package_label("foo@file:../foo"), "foo (local)");
    assert_eq!(short_package_label("foo@file:../foo.tgz"), "foo (tarball)");
    assert_eq!(short_package_label("foo@git+https://example.com/foo.git#main"), "foo (git)");
}

#[test]
fn parses_peer_and_patch_suffixes() {
    assert_eq!(parse_dep_path_name_version("foo@1.0.0(peer@2.0.0)"), Some(("foo", "1.0.0")));
    assert_eq!(parse_dep_path_name_version("@scope/foo@1.0.0"), Some(("@scope/foo", "1.0.0")));
}

#[test]
fn recognizes_git_hosted_tarballs() {
    assert!(is_git_hosted_dep_path("foo@https://codeload.github.com/user/foo/tar.gz/main"));
    assert!(!is_git_hosted_dep_path(
        "foo@https://codeload.github.com.example.com/user/foo/tar.gz/main"
    ));
}
