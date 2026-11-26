use super::{
    Compression,
    NarInfo,
};
use pretty_assertions::assert_eq;

/// The document `cache.nixos.org` serves for a Python 3.12 build.
const PYTHON: &str = include_str!("../fixtures/python3.narinfo");

#[test]
fn parses_a_document_from_the_nixos_cache() {
    let narinfo = NarInfo::parse(PYTHON).unwrap();
    assert_eq!(
        narinfo.store_path.to_string(),
        "/nix/store/v7wvwm1bpm6kzlqgw33m98arm7sw40ln-python3-3.12.15",
    );
    assert_eq!(narinfo.url, "nar/07hqdal6imym5wmbzccfywkr4px1fkyq8cklhasrnc3gvvh2jfv8.nar.zst");
    assert_eq!(narinfo.compression, Compression::Zstd);
    assert_eq!(narinfo.nar_size, 133_904_224);
    assert_eq!(narinfo.references.len(), 19);
    assert!(narinfo.references.contains(&narinfo.store_path));
    assert_eq!(narinfo.signatures.len(), 1);
}

#[test]
fn a_path_without_references_parses() {
    let text = PYTHON
        .lines()
        .filter(|line| !line.starts_with("References"))
        .collect::<Vec<_>>();
    assert_eq!(NarInfo::parse(&text.join("\n")).unwrap().references, []);
}

#[test]
fn rejects_a_url_that_leaves_the_cache() {
    for url in ["https://example.com/x.nar", "/etc/passwd", "nar/../../x.nar", ""] {
        let text = PYTHON.replace(
            "URL: nar/07hqdal6imym5wmbzccfywkr4px1fkyq8cklhasrnc3gvvh2jfv8.nar.zst",
            &format!("URL: {url}"),
        );
        NarInfo::parse(&text).unwrap_err();
    }
}

#[test]
fn rejects_a_missing_field_and_an_unknown_compression() {
    NarInfo::parse(&PYTHON.replace("NarHash: ", "Hash: ")).unwrap_err();
    NarInfo::parse(&PYTHON.replace("Compression: zstd", "Compression: br")).unwrap_err();
}
