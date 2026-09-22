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

/// A home-relative path is recognized only after normalization, so the
/// backslash spelling needs its own case: were the two ever reordered,
/// this form would be re-anchored under the workspace instead.
#[test]
fn leaves_a_backslash_spelled_home_relative_specifier_alone() {
    for (specifier, normalized) in
        [(r"file:~\tarballs\x.tgz", "file:~/tarballs/x.tgz"), (r"link:~\libs\x", "link:~/libs/x")]
    {
        assert_eq!(render(specifier, Some("packages/foo")), normalized);
        assert_eq!(render(specifier, None), normalized);
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
    assert_eq!(
        render_filesystem("../outside/x", Some("packages/foo")).as_deref(),
        Some("../../../outside/x"),
    );
}

/// Re-anchoring can drop a leading `./`, and a bare `<segment>/<segment>`
/// reads as a hosted-git shorthand rather than a path.
#[test]
fn keeps_a_reanchored_bare_path_unambiguously_local() {
    assert_eq!(render_filesystem("./libs/x", Some(".")).as_deref(), Some("./libs/x"));
}

/// A specifier naming the consuming package's own directory diffs to the
/// empty string, which reads as a missing path rather than as "here".
#[test]
fn renders_a_path_naming_the_consumer_as_here() {
    assert_eq!(render_filesystem("./packages/foo", Some("packages/foo")).as_deref(), Some("."));
    assert_eq!(render("file:./packages/foo", Some("packages/foo")), "file:.");
}

#[test]
fn declines_a_shape_that_need_not_be_a_local_path() {
    for specifier in ["user/repo", "^1.2.3", "npm:other@^1", "catalog:", "workspace:*"] {
        assert_eq!(render_filesystem(specifier, Some("packages/foo")), None, "{specifier}");
    }
}

/// The chain runs the local path resolver last, so a specifier that is
/// merely path-like has already been claimed by then: `repo.tgz`
/// resolves as a dist-tag, `user/repo.tgz` as a hosted-git shorthand,
/// and `c:pkg@1` as a single-letter named registry.
#[test]
fn declines_a_bare_specifier_another_resolver_claims() {
    for specifier in [
        "repo.tgz",
        "repo.tar.gz",
        "repo.tar.bz2",
        "repo.tbz2",
        "repo.tbz",
        "user/repo.tgz",
        "user/repo.tar.gz",
        "user/repo.tar.bz2",
        "user/repo",
        "c:pkg@1",
        "C:tools",
        "c:/abs/x.tgz",
        "c:/abs/x.tar.bz2",
    ] {
        assert_eq!(render_filesystem(specifier, Some("packages/foo")), None, "{specifier}");
    }
}

/// A tarball's protocol is unconditional, so naming it cannot change
/// how the package materializes. A directory's turns on whether the
/// dependency is injected, which the renderer cannot see, and an
/// explicit `link:` would outrank that and reference an injected
/// package in place instead of copying it.
#[test]
fn names_a_protocol_for_a_drive_anchored_path_only_when_it_is_free() {
    let drive = Path::new("C:/workspace");
    let render =
        |specifier| LocalSpec::parse_filesystem(specifier, drive).map(|spec| spec.render(None));
    assert_eq!(render("./deps/x.tgz").as_deref(), Some("file:C:/workspace/deps/x.tgz"));
    assert_eq!(render("./local-dep").as_deref(), Some("C:/workspace/local-dep"));
}

/// A path prefix is what lands a specifier on the local resolver, so a
/// tarball reached that way still moves.
#[test]
fn still_claims_a_path_prefixed_tarball() {
    assert_eq!(
        render_filesystem("./deps/repo.tgz", Some("packages/foo")).as_deref(),
        Some("../../deps/repo.tgz"),
    );
}

/// The resolver forward-slashes a specifier before it reads the path,
/// so a leading backslash names the filesystem root on every host. Read
/// as a raw `Path` off Windows it would look relative and be rebased
/// under the workspace instead.
#[test]
fn treats_a_leading_backslash_as_the_filesystem_root() {
    assert_eq!(render_filesystem(r"\foo", Some("packages/foo")).as_deref(), Some("/foo"));
    assert_eq!(render_filesystem(r"file:\foo", Some("packages/foo")).as_deref(), Some("file:/foo"));
}

/// A backslash inside a relative path is a separator to the resolver,
/// so the re-anchored form uses the same separators it would.
#[test]
fn reads_an_inner_backslash_as_a_separator() {
    assert_eq!(
        render_filesystem(r"file:.\deps\x.tgz", Some("packages/foo")).as_deref(),
        Some("file:../../deps/x.tgz"),
    );
}

/// A rooted path carries no drive prefix, so `Path::is_absolute` reads
/// it as relative on Windows and absolute everywhere else. A catalog
/// entry has to mean one thing on every host that installs the
/// workspace, so the expectation here is deliberately the same on all
/// of them.
#[test]
fn reads_a_rooted_path_the_same_way_on_every_host() {
    assert_eq!(
        render_filesystem("/foo/x.tgz", Some("packages/foo")).as_deref(),
        Some("/foo/x.tgz"),
    );
    assert_eq!(
        render_filesystem("file:/foo/x.tgz", Some("packages/foo")).as_deref(),
        Some("file:/foo/x.tgz"),
    );
    assert_eq!(render_filesystem(r"\foo", Some("packages/foo")).as_deref(), Some("/foo"));
}

/// The resolver strips the slashes after a protocol and then keeps a
/// dot, tilde or drive prefix relative, so `file:/./deps/x` resolves
/// from the project rather than the filesystem root — `pnpm install`
/// records it as `file:deps/x`. Anchoring shares that normalization
/// rather than repeating part of it.
#[test]
fn reads_a_post_protocol_slash_the_way_the_resolver_does() {
    assert_eq!(
        render_filesystem("file:/./deps/x", Some("packages/foo")).as_deref(),
        Some("file:../../deps/x"),
    );
    assert_eq!(
        render_filesystem(r"file:\.\deps\x", Some("packages/foo")).as_deref(),
        Some("file:../../deps/x"),
    );
    assert_eq!(
        render_filesystem("file://./deps/x", Some("packages/foo")).as_deref(),
        Some("file:../../deps/x"),
    );
}

/// Each step the [`normalize_specifier`] doc claims, checked against it.
#[test]
fn normalizes_a_specifier_as_documented() {
    use super::normalize_specifier;

    assert_eq!(normalize_specifier(r"deps\x"), "deps/x", "step 1: backslashes");
    assert_eq!(normalize_specifier("file:./deps/x"), "./deps/x", "step 2: protocol");
    assert_eq!(normalize_specifier("file:///C:/pkg"), "C:/pkg", "step 3: upper-case drive");
    assert_eq!(normalize_specifier("file:///c:/pkg"), "c:/pkg", "step 3: lower-case drive");
    assert_eq!(normalize_specifier("file:///abs/x"), "/abs/x", "step 4: slash restored");
    assert_eq!(normalize_specifier("file:/./deps/x"), "./deps/x", "step 4: dot stays relative");
    assert_eq!(normalize_specifier("file:/~/deps/x"), "~/deps/x", "step 4: tilde stays relative");
    assert_eq!(normalize_specifier("^1.2.3"), "^1.2.3", "no protocol: step 1 alone");
}

#[test]
fn recognizes_bzip2_tarball_extensions() {
    use super::is_tarball_filename;

    for name in ["pkg.tgz", "pkg.tar.gz", "pkg.tar", "pkg.tar.bz2", "pkg.tbz2", "pkg.tbz"] {
        assert!(is_tarball_filename(name), "expected {name} to be recognized as tarball");
    }
    for name in ["pkg.zip", "pkg.json", "pkg.js", "pkg"] {
        assert!(!is_tarball_filename(name), "expected {name} not to be recognized as tarball");
    }
}
