use super::short_package_label;
use pretty_assertions::assert_eq;

#[test]
fn labels_registry_packages_with_their_name() {
    assert_eq!(short_package_label("foo@1.0.0"), "foo");
    assert_eq!(short_package_label("@pnpm/test-skills@2.3.4"), "@pnpm/test-skills");
    assert_eq!(short_package_label("foo@1.0.0(patch_hash=gCgOF5)"), "foo");
}

#[test]
fn labels_prerelease_and_pruned_registry_versions_the_same() {
    assert_eq!(short_package_label("foo@1.0.0-beta.1"), "foo");
    assert_eq!(short_package_label("foo@1.0.0(peer_x@1.0.0)"), "foo");
}

#[test]
fn keeps_a_bare_package_name() {
    assert_eq!(short_package_label("foo"), "foo");
    assert_eq!(short_package_label("@pnpm/test-skills"), "@pnpm/test-skills");
}

#[test]
fn tags_git_hosted_packages() {
    assert_eq!(
        short_package_label(
            "mattpocock-skills@https://codeload.github.com/mattpocock/skills/tar.gz/a496f551"
        ),
        "mattpocock-skills (git)",
    );
    assert_eq!(
        short_package_label("skills@git+https://github.com/user/skills.git#v1.0.0"),
        "skills (git)",
    );
    assert_eq!(
        short_package_label("skills@git+ssh://git@github.com/user/skills.git(a1b2c3)"),
        "skills (git)",
    );
}

#[test]
fn tags_url_tarball_packages() {
    assert_eq!(short_package_label("pkg@https://example.com/pkg/pkg-1.0.0.tgz"), "pkg (tarball)");
    assert_eq!(
        short_package_label("pkg@https://example.com/get/pkg-1.0.0.tar.gz?ref=main"),
        "pkg (tarball)",
    );
    assert_eq!(
        short_package_label("pkg@https://example.com/get/pkg/archive.tar.gz(peer_a@1.0.0)"),
        "pkg (tarball)",
    );
}

#[test]
fn tells_local_directories_and_tarballs_apart() {
    assert_eq!(short_package_label("pkg@file:../lib"), "pkg (local)");
    assert_eq!(short_package_label("pkg@file:./skipped/pkg.tgz"), "pkg (tarball)");
    assert_eq!(short_package_label("pkg@file:./skipped/pkg.tar.gz"), "pkg (tarball)");
    assert_eq!(short_package_label("pkg@link:../lib"), "pkg (local)");
}
