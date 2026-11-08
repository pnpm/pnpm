use crate::Lockfile;
use pretty_assertions::assert_eq;
use std::{fmt::Write as _, path::Path};

fn parse(text: &str) -> Lockfile {
    Lockfile::parse(text, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile has content")
}

/// A lockfile written as JSON, as some tools and tests write it, is a
/// flow-style YAML document with no section blocks to split.
#[test]
fn a_flow_style_document_keeps_its_sections() {
    let block = parse(text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      is-odd:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
        ""
        "packages:"
        ""
        "  is-odd@1.0.0:"
        "    resolution: {integrity: sha512-AAAA}"
        ""
        "snapshots:"
        ""
        "  is-odd@1.0.0: {}"
    });
    let flow = parse(concat!(
        r#"{"lockfileVersion":"9.0","importers":{".":{"dependencies":{"is-odd":{"specifier":"^1.0.0","version":"1.0.0"}}}},"#,
        r#""packages":{"is-odd@1.0.0":{"resolution":{"integrity":"sha512-AAAA"}}},"snapshots":{"is-odd@1.0.0":{}}}"#,
    ));
    assert_eq!(flow, block);
    assert_eq!(block.importers.len(), 1);
    assert!(block.snapshots.is_some());
}

/// A section larger than the chunking threshold parses to what a whole parse
/// gives, with entries that straddle the chunk targets kept whole.
#[test]
fn a_large_section_parses_in_chunks() {
    let mut text = String::from("lockfileVersion: '9.0'\n\nimporters:\n\n  .: {}\n\npackages:\n\n");
    let mut snapshots = String::from("snapshots:\n\n");
    for index in 0..6000 {
        write!(
            text,
            "  pkg-{index}@1.0.0:\n    resolution: {{integrity: sha512-{index:064}}}\n    engines: {{node: '>=18'}}\n\n",
        )
        .expect("write to a string");
        write!(snapshots, "  pkg-{index}@1.0.0:\n    dependencies:\n      other: 1.0.0\n\n")
            .expect("write to a string");
    }
    text.push_str(&snapshots);
    assert!(text.len() > 2 * super::CHUNKED_SECTION_BYTES);
    let chunked = parse(&text);
    let whole: Lockfile = serde_saphyr::from_str(&text).expect("parse whole");
    assert_eq!(chunked, whole);
    assert_eq!(chunked.packages.as_ref().map(std::collections::HashMap::len), Some(6000));
}
