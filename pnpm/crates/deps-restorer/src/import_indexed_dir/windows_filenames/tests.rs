use super::{sanitize_component, sanitize_filenames};
use std::{collections::HashMap, path::PathBuf};

fn entries(names: &[&str]) -> HashMap<String, PathBuf> {
    names
        .iter()
        .map(|name| ((*name).to_string(), PathBuf::from(name)))
        .collect()
}

#[test]
fn sanitizes_invalid_components_and_preserves_valid_names() {
    let input = entries(&[
        "package.json",
        "assets?/icon.svg?as=metadata.d.ts",
        "letter-\u{e9}.txt",
        "literal~name.txt",
        "test~1.txt",
    ]);
    let sanitized = sanitize_filenames(&input).unwrap();
    assert_eq!(sanitized.paths["package.json"], PathBuf::from("package.json"));
    assert_eq!(
        sanitized.paths["assets/icon.svgas=metadata.d.ts"],
        PathBuf::from("assets?/icon.svg?as=metadata.d.ts"),
    );
    assert_eq!(sanitized.renamed, ["assets?/icon.svg?as=metadata.d.ts"]);
    assert!(sanitized.paths.contains_key("letter-\u{e9}.txt"));
    assert!(sanitized.paths.contains_key("literal~name.txt"));
    assert!(sanitized.paths.contains_key("test~1.txt"));
    assert!(sanitize_filenames(&entries(&["package.json", "assets/icon.svg"])).is_none());
}

#[test]
fn sanitizes_c1_controls_without_other_invalid_characters() {
    let input = entries(&["assets/name\u{80}\u{9f}.txt"]);
    let sanitized = sanitize_filenames(&input).unwrap();
    assert_eq!(sanitized.paths["assets/name.txt"], PathBuf::from("assets/name\u{80}\u{9f}.txt"));
    assert_eq!(sanitized.renamed, ["assets/name\u{80}\u{9f}.txt"]);
}

#[test]
fn sanitizes_collisions_and_empty_components() {
    let input = entries(&["name?.txt", "name.txt", "?/file", "longfilename.txt", "LONG~LMQ?.TXT"]);
    let sanitized = sanitize_filenames(&input).unwrap();
    assert_eq!(sanitized.paths.len(), 4);
    assert!(sanitized.paths.contains_key("name.txt"));
    assert_eq!(sanitized.paths["file"], PathBuf::from("?/file"));
    assert!(sanitized.paths.contains_key("longfilename.txt"));
    assert!(sanitized.paths.contains_key("LONG~LMQ.TXT"));
    assert!(sanitize_filenames(&entries(&["?"])).is_none());
}

#[test]
fn component_sanitization_matches_pnpm_11() {
    assert_eq!(sanitize_component("COM0.txt"), "");
    assert_eq!(sanitize_component("name\u{80}?.txt"), "name.txt");
    assert_eq!(sanitize_component(&"\u{e9}".repeat(128)), "\u{e9}".repeat(127));
}
