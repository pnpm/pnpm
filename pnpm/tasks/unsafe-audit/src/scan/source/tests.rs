use super::{Kind, has_safety_comment, kind, opens_unsafe, opts_out};
use text_block_macros::text_block_fnl;

/// Drive [`super::occurrences`] over `source`, written to a scratch file.
///
/// A directory of its own per call: `cargo test` runs every test of a
/// binary in one process, so a scratch path shared between them is one
/// test writing the source another is reading.
fn occurrences_in(source: &str) -> Vec<super::Occurrence> {
    let directory = tempfile::tempdir().expect("create the scratch directory");
    let file = directory.path().join("source.rs");
    std::fs::write(&file, source).expect("write the scratch source");
    super::occurrences(directory.path(), &file)
}

#[test]
fn an_unsafe_block_opens_one() {
    assert!(opens_unsafe("    unsafe { libc::geteuid() }"));
    assert!(opens_unsafe("    unsafe {"));
    assert!(opens_unsafe("unsafe fn raw() {}"));
    assert!(opens_unsafe("unsafe impl Send for Handle {}"));
    assert!(opens_unsafe(r#"unsafe extern "C" {"#));
    assert!(opens_unsafe("    let value = unsafe { read() };"));
}

/// The word appears in plenty of lines that are not an operation, and a
/// scan that matched them would flag crates that hold no `unsafe` at all.
#[test]
fn the_word_alone_opens_nothing() {
    assert!(!opens_unsafe("#![allow(unsafe_code)]"));
    assert!(!opens_unsafe("pub fn default_unsafe_perm() -> bool {"));
    assert!(!opens_unsafe("/// `unsafe` is the crate's purpose"));
    assert!(!opens_unsafe("// SAFETY: the unsafe below is sound"));
    assert!(!opens_unsafe(r#"    let message = "refusing an unsafe target";"#));
    assert!(!opens_unsafe("    unsafe_perm: bool,"));
    assert!(!opens_unsafe(r#"#[expect(unsafe_code, reason = "...")]"#));
}

/// An `unsafe extern` block declares a foreign ABI, which only a `-sys`
/// crate may; an `unsafe extern "abi" fn` item defines a callback the
/// operating system calls, which any abstraction crate may.
mod only_a_block_declares_a_foreign_abi {
    use super::{Kind, kind};

    #[test]
    fn a_block_is_a_declaration() {
        assert_eq!(kind(r#"unsafe extern "C" {"#), Kind::ForeignAbiBlock);
        assert_eq!(kind("unsafe extern {"), Kind::ForeignAbiBlock);
        assert_eq!(kind(r#"    unsafe extern "system" {"#), Kind::ForeignAbiBlock);
    }

    #[test]
    fn a_signature_is_not() {
        assert_eq!(
            kind(r#"unsafe extern "system" fn dispatch(event: u32) -> BOOL {"#),
            Kind::ForeignSignature,
        );
        assert_eq!(
            kind(r#"    unsafe extern "C" fn handler(signal: i32);"#),
            Kind::ForeignSignature,
        );
        assert_eq!(
            kind(r#"    unsafe extern "system" fn(HANDLE, *mut u16) -> BOOL;"#),
            Kind::ForeignSignature,
        );
    }

    #[test]
    fn anything_else_is_an_operation() {
        assert_eq!(kind("    unsafe { libc::geteuid() }"), Kind::Operation);
        assert_eq!(kind("unsafe impl Send for Handle {}"), Kind::Operation);
    }
}

/// The formatter wraps a long `let` after the `=`, which puts the comment
/// two lines above the `unsafe` rather than one.
#[test]
fn a_wrapped_let_does_not_break_the_pairing() {
    assert!(has_safety_comment(&["// SAFETY: it holds", "    let linked ="]));
}

/// A line of Rust quoted as data is data: this module's own fixtures are
/// such lines, and so is a diagnostic that quotes one.
#[test]
fn a_token_inside_a_string_literal_opens_nothing() {
    assert!(
        occurrences_in(r#"    for line in ["unsafe extern \"C\" {", "unsafe {"] {"#).is_empty(),
    );
    assert!(occurrences_in(r#"        "    let value = unsafe { read() };","#).is_empty());
    assert!(occurrences_in(r#"    panic!("unsafe fn found");"#).is_empty());
}

/// A raw string takes no escapes, so a quote count cannot be read through
/// one: inside `r##"..."##` a lone `"#` leaves the count even while the
/// token is still inside the literal.
#[test]
fn a_token_inside_a_raw_string_opens_nothing() {
    assert!(occurrences_in(r##"        r#"unsafe extern "C" {"#,"##).is_empty());
    assert!(occurrences_in(r###"    let text = r##"a "# unsafe { b"##;"###).is_empty());
}

/// An escaped quote does not open or close a literal, so a token after one
/// is still inside the literal that holds it.
#[test]
fn an_escaped_quote_does_not_end_a_literal() {
    assert!(occurrences_in(r#"    let text = "a \" unsafe { b";"#).is_empty());
}

#[test]
fn a_safety_comment_directly_above_counts() {
    assert!(has_safety_comment(&["// SAFETY: it holds"]));
}

/// An attribute or a blank line often sits between the comment and the
/// block, and neither breaks the pairing.
#[test]
fn an_attribute_or_blank_line_between_does_not_break_the_pairing() {
    assert!(has_safety_comment(&["// SAFETY: it holds", "", "#[cfg(unix)]"]));
}

/// A safety argument that names every precondition rarely fits on one
/// line, so the check reads the whole comment block.
#[test]
fn a_multi_line_safety_comment_counts() {
    assert!(has_safety_comment(&[
        "// SAFETY: the pointer is to a stack local that outlives the",
        "// call, and the callee retains no pointer.",
    ]));
}

/// An unrelated comment block between the two is not the argument for this
/// block, so it does not count.
#[test]
fn an_unrelated_comment_block_between_breaks_the_pairing() {
    assert!(!has_safety_comment(&["// SAFETY: it holds", "", "let value = compute();"]));
}

#[test]
fn no_comment_at_all_is_no_pairing() {
    assert!(!has_safety_comment(&[]));
    assert!(!has_safety_comment(&["let value = compute();"]));
}

/// The inner attribute is what exempts a whole crate, and it is the only
/// thing that may: the crates allowed to hold `unsafe` are a fixed list,
/// so a line that merely mentions the attribute must not read as one.
#[test]
fn an_inner_attribute_opts_out() {
    assert!(opts_out("#![allow(unsafe_code)]"));
    assert!(opts_out(r#"#![allow(unsafe_code, reason = "declares the ABI")]"#));
    assert!(opts_out("#![expect(unsafe_code)]"));
    assert!(opts_out("    #![allow(unsafe_code)]"));
}

#[test]
fn nothing_else_opts_out() {
    assert!(!opts_out("//! The crate carries `#![allow(unsafe_code)]`."));
    assert!(!opts_out("/// Whether a crate root has `#![allow(unsafe_code)]`."));
    assert!(!opts_out(r##"    let attribute = "#![allow(unsafe_code)]";"##));
    assert!(!opts_out("#[allow(unsafe_code)]"));
    assert!(!opts_out("#![allow(clippy::pedantic)]"));
    assert!(!opts_out("#![forbid(unsafe_code)]"));
}

/// A dereference opens with the same `*` a block-comment continuation
/// does. This scan is the policy gate, so a line mistaken for prose would
/// carry its `unsafe` past both the scan and the `// SAFETY:` check.
#[test]
fn a_dereference_is_not_a_block_comment() {
    assert!(opens_unsafe("    *slot = unsafe { libc::geteuid() };"));
    assert!(opens_unsafe("    *pointer = unsafe { read(source) };"));
}

/// A block-comment continuation is prose either way it is written: a `*`
/// alone, a `*` before the text, or the `*/` that closes the comment.
#[test]
fn a_block_comment_continuation_opens_nothing() {
    assert!(!opens_unsafe("     * unsafe { read() }"));
    assert!(!opens_unsafe("     *"));
    assert!(!opens_unsafe("     */"));
    assert!(!opens_unsafe("    * unsafe {"));
}

/// A string that spans lines holds lines that read as code on their own.
/// This scan is the policy gate, so one mistaken for an operation fails the
/// audit over a crate that holds no `unsafe` at all.
#[test]
fn an_unsafe_inside_a_multiline_raw_string_opens_nothing() {
    let found = occurrences_in(text_block_fnl! {
        "fn main() {"
        r##"    let sample = r#""##
        "unsafe {"
        r##""#;"##
        "}"
    });
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn an_unsafe_inside_a_multiline_string_opens_nothing() {
    let found = occurrences_in(text_block_fnl! {
        "fn main() {"
        r#"    let sample = ""#
        "unsafe {"
        r#"";"#
        "}"
    });
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn an_unsafe_inside_a_block_comment_opens_nothing() {
    let found = occurrences_in(text_block_fnl! {
        "fn main() {"
        "    /*"
        "    unsafe { read() }"
        "    */"
        "}"
    });
    assert!(found.is_empty(), "{found:?}");
}

/// The counterpart of the three above: what closed on an earlier line must
/// not blank the code that follows it, which is the direction that would
/// let an `unsafe` through.
#[test]
fn an_unsafe_after_a_multiline_raw_string_still_opens_one() {
    let found = occurrences_in(text_block_fnl! {
        "fn main() {"
        r##"    let s = r#""##
        "not code"
        r##""#;"##
        "    unsafe { read() }"
        "}"
    });
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A string closed earlier on the line leaves an `unsafe` after it an
/// operation. The lexical pass blanks contents but keeps delimiters, so
/// anything counting quotes on the blanked line still sees an opener.
#[test]
fn an_unsafe_after_a_closed_raw_string_on_one_line_opens_one() {
    let found = occurrences_in(text_block_fnl! {
        "fn main() {"
        r##"    let v = r#"x"#; unsafe { read() }"##
        "}"
    });
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A `'"'` is one character literal, not the start of a string, so an
/// `unsafe` after it is an operation.
#[test]
fn an_unsafe_after_a_quote_character_literal_opens_one() {
    let found = occurrences_in(text_block_fnl! {
        "fn main() {"
        r#"    if c == '"' { unsafe { read() } }"#
        "}"
    });
    assert_eq!(found.len(), 1, "{found:?}");
}
