/// Matches message text independently of miette's line wrapping and continuation bars.
pub fn assert_diagnostic_contains(text: &str, expected: &str) {
    assert!(
        unwrap_diagnostic(text).contains(&unwrap_diagnostic(expected)),
        "expected {expected:?} in:\n{text}",
    );
}

fn unwrap_diagnostic(text: &str) -> String {
    text.replace('│', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}
