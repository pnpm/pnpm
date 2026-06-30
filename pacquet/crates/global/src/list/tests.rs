use super::glob_match;

#[test]
fn glob_matches_exact_and_wildcards() {
    assert!(glob_match("foo", "foo"));
    assert!(!glob_match("foo", "bar"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("@scope/*", "@scope/pkg"));
    assert!(!glob_match("@scope/*", "@other/pkg"));
    assert!(glob_match("foo*bar", "fooXYZbar"));
}
