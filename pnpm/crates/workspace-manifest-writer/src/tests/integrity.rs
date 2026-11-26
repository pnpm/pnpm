use super::run_allow_builds;

/// A `#` inside a quoted value is part of the value, not a comment, so
/// the replacement must not preserve it as one.
#[test]
fn allow_builds_migrates_a_quoted_value_containing_a_hash() {
    let out = run_allow_builds(Some("allowBuilds:\n  esbuild: \"a # b\"\n"), &[("esbuild", false)]);
    assert_eq!(out.as_deref(), Some("permissions:\n  esbuild:\n    build: false\n"));
}
