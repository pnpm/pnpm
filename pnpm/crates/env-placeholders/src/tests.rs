use super::{
    placeholder_ranges,
    placeholders,
};
use pretty_assertions::assert_eq;

/// Each placeholder of `text` as `(literal, backslashes, raw, body)`, and
/// the text after the last one.
fn parts_of(text: &str) -> (Vec<(&str, usize, &str, &str)>, &str) {
    let (found, tail) = placeholders(text);
    let parts = found
        .iter()
        .map(|placeholder| {
            (
                placeholder.literal(),
                placeholder.backslashes(),
                placeholder.raw(),
                placeholder.body(),
            )
        })
        .collect();
    (parts, tail)
}

#[test]
fn cuts_text_at_each_placeholder() {
    assert_eq!(
        parts_of("a${B}c${D-e}"),
        (vec![("a", 0, "${B}", "B"), ("c", 0, "${D-e}", "D-e")], ""),
    );
    assert_eq!(parts_of("é${日本}é"), (vec![("é", 0, "${日本}", "日本")], "é"));
}

#[test]
fn counts_the_backslashes_before_a_placeholder() {
    assert_eq!(parts_of(r"x\\${A}"), (vec![("x", 2, "${A}", "A")], ""));
    assert_eq!(parts_of(r"${A}\${B}"), (vec![("", 0, "${A}", "A"), ("", 1, "${B}", "B")], ""));
}

#[test]
fn leaves_text_that_starts_no_placeholder() {
    for text in ["$", "${", "${}", "${A", "$A}", "${A$B}", "${A{B}", r"\$"] {
        assert_eq!(parts_of(text), (vec![], text), "{text:?}");
    }
    assert_eq!(parts_of("${A${B}"), (vec![("${A", 0, "${B}", "B")], ""));
}

#[test]
fn ranges_skip_escaped_placeholders() {
    assert_eq!(placeholder_ranges(r"${A}\${B}${C}"), vec![0..4, 9..13]);
    assert_eq!(placeholder_ranges(r"\\${A}"), vec![2..6]);
    assert_eq!(placeholder_ranges("é${A}"), vec![2..6]);
}
