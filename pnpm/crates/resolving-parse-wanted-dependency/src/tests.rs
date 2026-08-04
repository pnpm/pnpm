use crate::{ParsedWantedDependency, is_valid_old_npm_package_name, parse_wanted_dependency};

fn parsed(alias: Option<&str>, bare: Option<&str>) -> ParsedWantedDependency {
    ParsedWantedDependency {
        alias: alias.map(str::to_owned),
        bare_specifier: bare.map(str::to_owned),
    }
}

#[test]
fn plain_name_without_specifier_returns_alias_only() {
    assert_eq!(parse_wanted_dependency("foo"), parsed(Some("foo"), None));
}

#[test]
fn scoped_name_without_specifier_returns_alias_only() {
    assert_eq!(parse_wanted_dependency("@scope/foo"), parsed(Some("@scope/foo"), None));
}

#[test]
fn plain_name_with_version_splits_on_at() {
    assert_eq!(parse_wanted_dependency("foo@1.2.3"), parsed(Some("foo"), Some("1.2.3")));
}

#[test]
fn scoped_name_with_version_splits_after_scope() {
    assert_eq!(
        parse_wanted_dependency("@scope/foo@1.2.3"),
        parsed(Some("@scope/foo"), Some("1.2.3")),
    );
}

#[test]
fn plain_name_with_tag_splits_on_at() {
    assert_eq!(parse_wanted_dependency("foo@latest"), parsed(Some("foo"), Some("latest")));
}

#[test]
fn npm_alias_form_keeps_inner_at_inside_bare_specifier() {
    // The `parse-wanted-dependency` function splits on the first `@` only;
    // the second `@` (inside `npm:lodash@^4`) survives in the bare specifier
    // and is later routed by the npm resolver's own alias parser.
    assert_eq!(
        parse_wanted_dependency("foo@npm:lodash@^4"),
        parsed(Some("foo"), Some("npm:lodash@^4")),
    );
}

#[test]
fn workspace_protocol_with_alias_splits() {
    assert_eq!(
        parse_wanted_dependency("foo@workspace:*"),
        parsed(Some("foo"), Some("workspace:*")),
    );
}

#[test]
fn git_ssh_url_keeps_whole_input_as_bare_specifier() {
    // `git+ssh://git@github.com/owner/repo` has an `@` after index 0, but
    // the prefix `git+ssh://git` is not a valid package name (contains `:`
    // and `/`), so the splitter declines and the full URL flows through
    // as a bare specifier.
    let input = "git+ssh://git@github.com/owner/repo";
    assert_eq!(parse_wanted_dependency(input), parsed(None, Some(input)));
}

#[test]
fn tarball_url_with_no_at_keeps_whole_input_as_bare_specifier() {
    let input = "https://example.com/foo.tgz";
    assert_eq!(parse_wanted_dependency(input), parsed(None, Some(input)));
}

#[test]
fn bare_version_range_keeps_whole_input_as_bare_specifier() {
    // `^1.2.3` is not a valid package name (caret isn't URL-safe), so
    // the no-`@` branch routes it to `bare_specifier`.
    assert_eq!(parse_wanted_dependency("^1.2.3"), parsed(None, Some("^1.2.3")));
}

#[test]
fn numeric_only_input_is_treated_as_an_alias() {
    // `1.2.3` happens to satisfy `validForOldPackages` (all URL-safe
    // characters, no leading dot/dash/underscore), so it parses as an
    // alias with no specifier. Mirrors upstream's behavior — a quirk
    // worth pinning so future refactors don't drift.
    assert_eq!(parse_wanted_dependency("1.2.3"), parsed(Some("1.2.3"), None));
}

#[test]
fn prefix_protocol_with_at_keeps_whole_input_as_bare_specifier() {
    // `pnpm:foo@npm:bar` — the substring before the first `@` is
    // `pnpm:foo`, which fails `validForOldPackages` (contains `:`),
    // so the whole string flows through as a bare specifier.
    let input = "pnpm:foo@npm:bar";
    assert_eq!(parse_wanted_dependency(input), parsed(None, Some(input)));
}

#[test]
fn empty_specifier_after_at_yields_empty_bare_specifier() {
    // `foo@` is a degenerate split: alias is the valid name `foo`,
    // bare specifier is the empty string. Upstream returns the same
    // shape; pacquet pins it so the dispatcher downstream can treat
    // an empty bare specifier as "default tag" the same way pnpm does.
    assert_eq!(parse_wanted_dependency("foo@"), parsed(Some("foo"), Some("")));
}

#[test]
fn is_valid_old_npm_package_name_accepts_common_shapes() {
    for ok in ["foo", "foo-bar", "foo.bar", "foo_bar", "@scope/foo", "Foo", "1.2.3"] {
        assert!(is_valid_old_npm_package_name(ok), "{ok} should be valid");
    }
}

#[test]
fn is_valid_old_npm_package_name_rejects_error_cases() {
    // These are the exact cases that flip `errors` from empty under
    // `validate-npm-package-name@7`; see the rule list in
    // [`is_valid_old_npm_package_name`].
    for bad in [
        "",                 // empty
        ".foo",             // leading dot
        "_foo",             // leading underscore
        "-foo",             // leading hyphen
        " foo",             // leading whitespace
        "foo ",             // trailing whitespace
        "node_modules",     // exclusion list
        "Node_Modules",     // exclusion list, case-insensitive
        "favicon.ico",      // exclusion list
        "foo bar",          // space inside (not URL-safe)
        "foo/bar",          // unscoped slash
        "@scope/.foo",      // scoped, but bare half starts with `.`
        "pnpm:foo",         // colon (not URL-safe, not a scoped shape)
        "^1.2.3",           // caret (not URL-safe)
        "@scope/foo/extra", // scoped shape with extra slash
        "@/foo",            // scoped shape with empty user
        "@scope/",          // scoped shape with empty pkg
    ] {
        assert!(!is_valid_old_npm_package_name(bad), "{bad:?} should be invalid");
    }
}
