use super::{EnvVar, env_replace_lossy};
use pretty_assertions::assert_eq;

/// Empty env: no variable is ever set. Used by tests that only
/// exercise the literal-passthrough or escape paths.
struct NoEnv;
impl EnvVar for NoEnv {
    fn var(_: &str) -> Option<String> {
        None
    }
}

/// Run [`env_replace_lossy`] and assert no placeholder went unresolved.
/// Used by tests that exercise paths where every placeholder must expand.
fn replace_clean<Sys: EnvVar>(text: &str) -> String {
    let (value, unresolved) = env_replace_lossy::<Sys>(text);
    assert_eq!(unresolved, Vec::<String>::new(), "unexpected unresolved placeholders");
    value
}

#[test]
fn substitutes_simple_placeholder() {
    struct EnvWithToken;
    impl EnvVar for EnvWithToken {
        fn var(name: &str) -> Option<String> {
            (name == "TOKEN").then(|| "abc123".to_owned())
        }
    }
    assert_eq!(replace_clean::<EnvWithToken>("Bearer ${TOKEN}"), "Bearer abc123");
}

#[test]
fn unresolved_drops_to_empty_and_collects_placeholder() {
    let (value, unresolved) = env_replace_lossy::<NoEnv>("//reg/:_authToken=${MISSING}");
    assert_eq!(value, "//reg/:_authToken=");
    assert_eq!(unresolved, vec!["${MISSING}".to_owned()]);
}

#[test]
fn uses_default_when_variable_unset() {
    assert_eq!(replace_clean::<NoEnv>("${MISSING:-fallback}"), "fallback");
}

#[test]
fn uses_default_when_variable_empty() {
    struct EmptyEnv;
    impl EnvVar for EmptyEnv {
        fn var(name: &str) -> Option<String> {
            (name == "EMPTY").then(String::new)
        }
    }
    assert_eq!(replace_clean::<EmptyEnv>("${EMPTY:-fallback}"), "fallback");
}

#[test]
fn variable_wins_over_default_when_set() {
    struct EnvWithPort;
    impl EnvVar for EnvWithPort {
        fn var(name: &str) -> Option<String> {
            (name == "PORT").then(|| "8080".to_owned())
        }
    }
    assert_eq!(replace_clean::<EnvWithPort>("${PORT:-3000}"), "8080");
}

#[test]
fn passthrough_when_no_placeholder() {
    assert_eq!(replace_clean::<NoEnv>("plain string"), "plain string");
}

#[test]
fn lone_dollar_is_left_alone() {
    assert_eq!(replace_clean::<NoEnv>("$ price"), "$ price");
}

/// Hits the early-`None` branch of `bytes.get(start + 1)?` inside
/// [`find_placeholder_end`]: the input ends on a `$` with no byte
/// to peek at.
#[test]
fn trailing_dollar_with_no_byte_after_is_passthrough() {
    assert_eq!(replace_clean::<NoEnv>("$"), "$");
    assert_eq!(replace_clean::<NoEnv>("foo$"), "foo$");
}

/// A nested `$` or `{` inside a placeholder body, or an unclosed
/// `${...`, leaves the input verbatim. [`NoEnv`] would also return
/// `None` for any lookup, but the parser must short-circuit
/// *before* the lookup, so swapping in a richer fake would still
/// be a no-op.
#[test]
fn malformed_placeholder_is_left_alone() {
    assert_eq!(replace_clean::<NoEnv>("${OPEN"), "${OPEN");
    assert_eq!(replace_clean::<NoEnv>("${A$B}"), "${A$B}");
}

/// One literal backslash escapes the placeholder; the parser
/// must skip the var lookup entirely. [`NoEnv`] is sufficient
/// because the lookup never runs in this branch.
#[test]
fn odd_backslash_count_escapes_placeholder() {
    assert_eq!(replace_clean::<NoEnv>(r"\${X}"), "${X}");
}

#[test]
fn even_backslash_count_keeps_half_and_substitutes() {
    struct EnvWithX;
    impl EnvVar for EnvWithX {
        fn var(name: &str) -> Option<String> {
            (name == "X").then(|| "y".to_owned())
        }
    }
    assert_eq!(replace_clean::<EnvWithX>(r"\\${X}"), r"\y");
}

#[test]
fn handles_multiple_placeholders() {
    // `static` scenario data inside the test fn matches the
    // pattern from
    // [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339):
    // keep the fake stateless by stashing variation in a `static`,
    // not in `&self`.
    static ENV: &[(&str, &str)] = &[("A", "1"), ("B", "2")];
    struct StaticEnv;
    impl EnvVar for StaticEnv {
        fn var(name: &str) -> Option<String> {
            ENV.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
        }
    }
    assert_eq!(replace_clean::<StaticEnv>("${A}-${B}-${A}"), "1-2-1");
}

/// The source-backslash count must come from the original input,
/// not from the working `output` buffer. Without that, a
/// previously-expanded variable whose value ends in `\` would be
/// conflated with literal source `\` characters preceding the
/// next `${...}`. Upstream's regex `(?<!\\)(\\*)\$\{...}` runs on
/// the source, so a single literal `\` between two placeholders
/// must escape only the second one regardless of any trailing `\`
/// in the first's value.
#[test]
fn backslash_count_uses_source_not_output_buffer() {
    struct Env;
    impl EnvVar for Env {
        fn var(name: &str) -> Option<String> {
            match name {
                "A" => Some(r"x\".to_owned()),
                "B" => Some("should-not-expand".to_owned()),
                _ => None,
            }
        }
    }
    assert_eq!(replace_clean::<Env>(r"${A}\${B}"), r"x\${B}");
}

#[test]
fn placeholder_inside_url() {
    // The actual .npmrc shape pnpm users hit.
    struct EnvWithToken;
    impl EnvVar for EnvWithToken {
        fn var(name: &str) -> Option<String> {
            (name == "NPM_TOKEN").then(|| "secret".to_owned())
        }
    }
    assert_eq!(
        replace_clean::<EnvWithToken>("//registry.npmjs.org/:_authToken=${NPM_TOKEN}"),
        "//registry.npmjs.org/:_authToken=secret",
    );
}

#[test]
fn preserves_resolved_and_default_placeholders_alongside_unresolved() {
    struct EnvWithSet;
    impl EnvVar for EnvWithSet {
        fn var(name: &str) -> Option<String> {
            (name == "SET").then(|| "AAA".to_owned())
        }
    }
    let (value, unresolved) =
        env_replace_lossy::<EnvWithSet>("${SET}-${UNSET}-${DEFAULTED:-fallback}");
    assert_eq!(value, "AAA--fallback");
    assert_eq!(unresolved, vec!["${UNSET}".to_owned()]);
}

#[test]
fn collects_every_unresolved_placeholder_occurrence() {
    let (value, unresolved) = env_replace_lossy::<NoEnv>("${A}-${B}-${A}");
    assert_eq!(value, "--");
    assert_eq!(unresolved, vec!["${A}".to_owned(), "${B}".to_owned(), "${A}".to_owned()]);
}
