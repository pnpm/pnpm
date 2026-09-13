use super::edit;

/// Whether `value` holds a character YAML treats as a line break: a
/// control character (newline, carriage return, ...) or one of the Unicode
/// line/paragraph separators, which are not in the control category.
///
/// The block-style writers splice `value` into a single `key: value` /
/// `- item` line. A control character forces a multi-line scalar and
/// corrupts the document outright; a separator is subtler — the emitter
/// folds the scalar and the parser reads back the folding indentation as
/// part of the value, so the write silently succeeds with a mangled
/// value. The values these writers handle (GHSA ids, version-policy
/// specs, override selectors/specifiers, catalog names) never
/// legitimately contain either.
pub(super) fn has_control_char(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
}

/// The first of `paths` whose value is written as an inline shape none of
/// the writers can edit, named for the error message. A single-line flow
/// collection is editable and never reported here; a multi-line one, an
/// alias, or a scalar standing where a collection belongs is.
pub(super) fn unsupported_inline_key(text: &str, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find(|path| edit::has_unsupported_inline_value(text, path))
        .map(|path| path.join("."))
}
