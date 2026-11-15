use super::SentinelPlaceholders;

#[test]
fn try_restore_decodes_plain_single_and_double_quoted_scalar_styles() {
    let text = r#"plain: ${A:-x\"y''z}
single: '${B:-x''y\nz}'
double: "${C:-a\"b\\c\td\x21\u003f}"
"#;
    let sentinels = SentinelPlaceholders::new(text).unwrap();
    let token = |marker: &str, index: usize| format!("{}{marker}{index}_", sentinels.prefix);
    assert_eq!(
        sentinels
            .try_restore(text, &token(r"\/''", 0))
            .as_deref(),
        Some(r#"${A:-x\"y''z}"#),
    );
    assert_eq!(
        sentinels
            .try_restore(text, &token(r"\/'", 1))
            .as_deref(),
        Some(r"${B:-x'y\nz}"),
    );
    assert_eq!(
        sentinels
            .try_restore(text, &token("/''", 2))
            .as_deref(),
        Some("${C:-a\"b\\c\td!?}"),
    );
}

#[test]
fn try_restore_rejects_unescaped_quotes_and_invalid_double_quote_escapes() {
    let text = r#"${A:-bad"quote} ${B:-bad'quote} ${C:-bad\qescape}"#;
    let sentinels = SentinelPlaceholders::new(text).unwrap();
    let token = |marker: &str, index: usize| format!("{}{marker}{index}_", sentinels.prefix);
    assert!(
        sentinels
            .try_restore(text, &token("/''", 0))
            .is_none(),
    );
    assert!(
        sentinels
            .try_restore(text, &token(r"\/'", 1))
            .is_none(),
    );
    assert!(
        sentinels
            .try_restore(text, &token("/''", 2))
            .is_none(),
    );
}
