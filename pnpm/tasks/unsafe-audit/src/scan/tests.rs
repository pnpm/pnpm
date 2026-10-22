use super::{Kind, has_safety_comment, kind, opens_unsafe, opts_out};

#[test]
fn an_unsafe_block_opens_one() {
    for line in [
        "    unsafe { libc::geteuid() }",
        "    unsafe {",
        "unsafe fn raw() {}",
        "unsafe impl Send for Handle {}",
        r#"unsafe extern "C" {"#,
        "    let value = unsafe { read() };",
    ] {
        assert!(opens_unsafe(line), "{line}");
    }
}

/// The word appears in plenty of lines that are not an operation, and a
/// scan that matched them would flag crates that hold no `unsafe` at all.
#[test]
fn the_word_alone_opens_nothing() {
    for line in [
        "#![allow(unsafe_code)]",
        "pub fn default_unsafe_perm() -> bool {",
        "/// `unsafe` is the crate's purpose",
        "// SAFETY: the unsafe below is sound",
        r#"    let message = "refusing an unsafe target";"#,
        "    unsafe_perm: bool,",
        r#"#[expect(unsafe_code, reason = "...")]"#,
    ] {
        assert!(!opens_unsafe(line), "{line}");
    }
}

/// An `unsafe extern` block declares a foreign ABI, which only a `-sys`
/// crate may; an `unsafe extern "abi" fn` item defines a callback the
/// operating system calls, which any abstraction crate may.
#[test]
fn only_a_block_declares_a_foreign_abi() {
    for line in [r#"unsafe extern "C" {"#, "unsafe extern {", r#"    unsafe extern "system" {"#] {
        assert_eq!(kind(line), Kind::ForeignAbiBlock, "{line}");
    }
    for line in [
        r#"unsafe extern "system" fn dispatch(event: u32) -> BOOL {"#,
        r#"    unsafe extern "C" fn handler(signal: i32);"#,
        r#"    unsafe extern "system" fn(HANDLE, *mut u16) -> BOOL;"#,
    ] {
        assert_eq!(kind(line), Kind::ForeignSignature, "{line}");
    }
    for line in ["    unsafe { libc::geteuid() }", "unsafe impl Send for Handle {}"] {
        assert_eq!(kind(line), Kind::Operation, "{line}");
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
    for line in [
        r#"    for line in ["unsafe extern \"C\" {", "unsafe {"] {"#,
        r#"        "    let value = unsafe { read() };","#,
        r#"    panic!("unsafe fn found");"#,
    ] {
        assert!(!opens_unsafe(line), "{line}");
    }
}

/// A raw string takes no escapes, so the quote count cannot be read
/// through one: inside `r##"..."##` a lone `"#` leaves the count even
/// while the token is still inside the literal.
#[test]
fn a_token_inside_a_raw_string_opens_nothing() {
    for line in
        [r##"        r#"unsafe extern "C" {"#,"##, r###"    let text = r##"a "# unsafe { b"##;"###]
    {
        assert!(!opens_unsafe(line), "{line}");
    }
}

/// An escaped quote does not open or close a literal, so a token after one
/// is still inside the literal that holds it.
#[test]
fn an_escaped_quote_does_not_end_a_literal() {
    assert!(!opens_unsafe(r#"    let text = "a \" unsafe { b";"#));
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
    for line in [
        "#![allow(unsafe_code)]",
        r#"#![allow(unsafe_code, reason = "declares the ABI")]"#,
        "#![expect(unsafe_code)]",
        "    #![allow(unsafe_code)]",
    ] {
        assert!(opts_out(line), "{line}");
    }
}

#[test]
fn nothing_else_opts_out() {
    for line in [
        "//! The crate carries `#![allow(unsafe_code)]`.",
        "/// Whether a crate root has `#![allow(unsafe_code)]`.",
        r##"    let attribute = "#![allow(unsafe_code)]";"##,
        "#[allow(unsafe_code)]",
        "#![allow(clippy::pedantic)]",
        "#![forbid(unsafe_code)]",
    ] {
        assert!(!opts_out(line), "{line}");
    }
}
