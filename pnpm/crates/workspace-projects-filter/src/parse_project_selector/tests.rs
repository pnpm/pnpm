use crate::parse_project_selector::{ProjectSelector, parse_project_selector};
use std::path::{Path, PathBuf};

const PREFIX: &str = "/prefix";

fn parse(raw: &str) -> ProjectSelector {
    parse_project_selector(raw, Path::new(PREFIX))
}

fn name(pattern: &str) -> Option<String> {
    Some(pattern.to_string())
}

fn dir(path: &str) -> Option<PathBuf> {
    Some(PathBuf::from(path))
}

#[test]
fn plain_name() {
    assert_eq!(parse("foo"), ProjectSelector { name_pattern: name("foo"), ..Default::default() });
}

#[test]
fn name_with_dependencies() {
    assert_eq!(
        parse("foo..."),
        ProjectSelector {
            include_dependencies: true,
            name_pattern: name("foo"),
            ..Default::default()
        },
    );
}

#[test]
fn name_with_dependents() {
    assert_eq!(
        parse("...foo"),
        ProjectSelector {
            include_dependents: true,
            name_pattern: name("foo"),
            ..Default::default()
        },
    );
}

#[test]
fn name_with_dependencies_and_dependents() {
    assert_eq!(
        parse("...foo..."),
        ProjectSelector {
            include_dependencies: true,
            include_dependents: true,
            name_pattern: name("foo"),
            ..Default::default()
        },
    );
}

#[test]
fn name_with_dependencies_excluding_self() {
    assert_eq!(
        parse("foo^..."),
        ProjectSelector {
            exclude_self: true,
            include_dependencies: true,
            name_pattern: name("foo"),
            ..Default::default()
        },
    );
}

#[test]
fn name_with_dependents_excluding_self() {
    assert_eq!(
        parse("...^foo"),
        ProjectSelector {
            exclude_self: true,
            include_dependents: true,
            name_pattern: name("foo"),
            ..Default::default()
        },
    );
}

#[test]
fn relative_path_selector() {
    assert_eq!(
        parse("./foo"),
        ProjectSelector { parent_dir: dir("/prefix/foo"), ..Default::default() },
    );
}

#[test]
fn parent_relative_path_selector() {
    assert_eq!(parse("../foo"), ProjectSelector { parent_dir: dir("/foo"), ..Default::default() });
}

#[test]
fn dependents_of_brace_dir() {
    assert_eq!(
        parse("...{./foo}"),
        ProjectSelector {
            include_dependents: true,
            parent_dir: dir("/prefix/foo"),
            ..Default::default()
        },
    );
}

#[test]
fn absolute_brace_dir_extends_prefix() {
    // Node's `path.join` concatenates an absolute segment instead of
    // letting it reset the prefix (`path.join('/prefix', '/pkg')` ->
    // `/prefix/pkg`), so an absolute directory selector resolves under
    // the workspace prefix.
    assert_eq!(
        parse("{/pkg}"),
        ProjectSelector { parent_dir: dir("/prefix/pkg"), ..Default::default() },
    );
}

#[test]
fn dot_selects_prefix() {
    assert_eq!(parse("."), ProjectSelector { parent_dir: dir("/prefix"), ..Default::default() });
}

#[test]
fn dotdot_selects_parent_of_prefix() {
    assert_eq!(parse(".."), ProjectSelector { parent_dir: dir("/"), ..Default::default() });
}

#[test]
fn diff_selector() {
    assert_eq!(
        parse("[master]"),
        ProjectSelector { diff: Some("master".to_string()), ..Default::default() },
    );
}

#[test]
fn brace_and_diff() {
    assert_eq!(
        parse("{foo}[master]"),
        ProjectSelector {
            diff: Some("master".to_string()),
            parent_dir: dir("/prefix/foo"),
            ..Default::default()
        },
    );
}

#[test]
fn name_brace_and_diff() {
    assert_eq!(
        parse("pattern{foo}[master]"),
        ProjectSelector {
            diff: Some("master".to_string()),
            name_pattern: name("pattern"),
            parent_dir: dir("/prefix/foo"),
            ..Default::default()
        },
    );
}

#[test]
fn diff_with_dependencies() {
    assert_eq!(
        parse("[master]..."),
        ProjectSelector {
            diff: Some("master".to_string()),
            include_dependencies: true,
            ..Default::default()
        },
    );
}

#[test]
fn diff_with_dependents() {
    assert_eq!(
        parse("...[master]"),
        ProjectSelector {
            diff: Some("master".to_string()),
            include_dependents: true,
            ..Default::default()
        },
    );
}

#[test]
fn diff_with_dependencies_and_dependents() {
    assert_eq!(
        parse("...[master]..."),
        ProjectSelector {
            diff: Some("master".to_string()),
            include_dependencies: true,
            include_dependents: true,
            ..Default::default()
        },
    );
}

// The upstream regex name group `[^.][^{}[\]]*` lets the name's first
// char be a brace/bracket; the parser backtracks the greedy name to let
// `{...}` / `[...]` match. These cases (malformed or unusual selectors) must
// resolve the same way the regex does, including keeping `!`/`...`
// modifiers that the name-fallback path would otherwise drop.

#[test]
fn exclude_with_leading_brace_name_keeps_exclude() {
    assert_eq!(
        parse("!{foo"),
        ProjectSelector { exclude: true, name_pattern: name("{foo"), ..Default::default() },
    );
}

#[test]
fn leading_brace_name_then_diff() {
    assert_eq!(
        parse("{[master]"),
        ProjectSelector {
            diff: Some("master".to_string()),
            name_pattern: name("{"),
            ..Default::default()
        },
    );
}

#[test]
fn leading_brace_name_then_dir() {
    assert_eq!(
        parse("}foo{bar}"),
        ProjectSelector {
            name_pattern: name("}foo"),
            parent_dir: dir("/prefix/bar"),
            ..Default::default()
        },
    );
}

#[test]
fn unparsable_braces_fall_back_to_name() {
    assert_eq!(
        parse("foo}bar"),
        ProjectSelector { name_pattern: name("foo}bar"), ..Default::default() },
    );
}

#[test]
fn triple_dots_reduces_to_dependencies_only() {
    assert_eq!(parse("..."), ProjectSelector { include_dependencies: true, ..Default::default() });
}

#[test]
fn empty_braces_fall_back_to_name() {
    assert_eq!(parse("{}"), ProjectSelector { name_pattern: name("{}"), ..Default::default() });
}

#[test]
fn dot_prefixed_name_is_not_a_location() {
    assert_eq!(parse(".foo"), ProjectSelector { name_pattern: name(".foo"), ..Default::default() });
}
