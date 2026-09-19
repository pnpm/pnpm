use std::path::Path;

use pretty_assertions::assert_eq;

use super::LocalSpec;

#[cfg(windows)]
const ROOT: &str = r"C:\workspace";
#[cfg(not(windows))]
const ROOT: &str = "/workspace";

fn root() -> &'static Path {
    Path::new(ROOT)
}

fn render(specifier: &str, consumer: Option<&str>) -> String {
    let consumer = consumer.map(|dir| root().join(dir));
    LocalSpec::parse(specifier, root()).expect("a local specifier").render(consumer.as_deref())
}

#[test]
fn declines_specifiers_without_a_local_protocol() {
    for specifier in ["^1.2.3", "catalog:", "workspace:*", "npm:foo@1", "./foo", "https://x/y.tgz"]
    {
        assert_eq!(LocalSpec::parse(specifier, root()), None, "{specifier}");
    }
}

#[test]
fn reanchors_a_relative_path_against_the_consumer() {
    assert_eq!(render("file:./tarballs/x.tgz", Some("packages/foo")), "file:../../tarballs/x.tgz");
    assert_eq!(render("link:libs/x", Some("packages/foo")), "link:../../libs/x");
}

#[test]
fn reanchors_a_relative_path_that_climbs_above_the_base() {
    assert_eq!(
        render("file:../outside/x.tgz", Some("packages/foo")),
        "file:../../../outside/x.tgz",
    );
}

#[test]
fn keeps_a_relative_path_unchanged_for_a_consumer_in_the_base_directory() {
    assert_eq!(render("file:./tarballs/x.tgz", Some(".")), "file:tarballs/x.tgz");
}

#[test]
fn renders_the_consumers_own_directory_as_a_dot() {
    assert_eq!(render("link:packages/foo", Some("packages/foo")), "link:.");
}

#[test]
fn renders_an_absolute_path_without_a_consumer_directory() {
    let expected = format!("file:{}/tarballs/x.tgz", ROOT.replace('\\', "/"));
    assert_eq!(render("file:./tarballs/x.tgz", None), expected);
}

#[test]
fn leaves_an_absolute_specifier_alone() {
    let absolute = format!("file:{}/tarballs/x.tgz", ROOT.replace('\\', "/"));
    assert_eq!(render(&absolute, Some("packages/foo")), absolute);
}

#[test]
fn leaves_a_home_relative_specifier_alone() {
    for specifier in ["file:~/tarballs/x.tgz", "link:~/libs/x"] {
        assert_eq!(render(specifier, Some("packages/foo")), specifier);
        assert_eq!(render(specifier, None), specifier);
    }
}

fn render_filesystem(specifier: &str, consumer: Option<&str>) -> Option<String> {
    let consumer = consumer.map(|dir| root().join(dir));
    LocalSpec::parse_filesystem(specifier, root()).map(|spec| spec.render(consumer.as_deref()))
}

#[test]
fn reanchors_a_bare_path_the_way_it_reanchors_the_protocol_form() {
    assert_eq!(
        render_filesystem("./tarballs/x.tgz", Some("packages/foo")).as_deref(),
        Some("../../tarballs/x.tgz"),
    );
    assert_eq!(render_filesystem("x.tgz", Some("packages/foo")).as_deref(), Some("../../x.tgz"));
}

/// Re-anchoring can drop a leading `./`, and a bare `<segment>/<segment>`
/// reads as a hosted-git shorthand rather than a path.
#[test]
fn keeps_a_reanchored_bare_path_unambiguously_local() {
    assert_eq!(render_filesystem("./libs/x", Some(".")).as_deref(), Some("./libs/x"));
}

#[test]
fn declines_a_shape_that_need_not_be_a_local_path() {
    for specifier in ["user/repo", "^1.2.3", "npm:other@^1", "catalog:", "workspace:*"] {
        assert_eq!(render_filesystem(specifier, Some("packages/foo")), None, "{specifier}");
    }
}

/// A bare specifier the resolver chain reaches through a resolver other
/// than the local one must not be re-anchored as a path: `c:pkg@1` is a
/// single-letter named registry as much as a drive path, and
/// `user/repo.tgz` is a hosted-git shorthand.
#[test]
fn declines_a_bare_specifier_another_resolver_claims() {
    for specifier in
        ["c:pkg@1", "C:tools", "c:/abs/x.tgz", "user/repo.tgz", "user/repo.tar.gz", "user/repo"]
    {
        assert_eq!(render_filesystem(specifier, Some("packages/foo")), None, "{specifier}");
    }
}

/// A tarball name with no slash cannot be a hosted-git shorthand, which
/// needs an owner segment, so it stays claimed.
#[test]
fn still_claims_a_slash_free_tarball_name() {
    assert_eq!(
        render_filesystem("repo.tgz", Some("packages/foo")).as_deref(),
        Some("../../repo.tgz"),
    );
    assert_eq!(
        render_filesystem("./deps/repo.tgz", Some("packages/foo")).as_deref(),
        Some("../../deps/repo.tgz"),
    );
}
