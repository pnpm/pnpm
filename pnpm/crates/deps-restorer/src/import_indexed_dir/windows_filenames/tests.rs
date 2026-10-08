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
    let sanitized = sanitize_filenames(&input).unwrap().unwrap();
    assert_eq!(sanitized.paths["package.json"], PathBuf::from("package.json"));
    assert_eq!(
        sanitized.paths["assets/icon.svgas=metadata.d.ts"],
        PathBuf::from("assets?/icon.svg?as=metadata.d.ts"),
    );
    assert_eq!(sanitized.renamed, ["assets?/icon.svg?as=metadata.d.ts"]);
    assert!(sanitized.paths.contains_key("letter-\u{e9}.txt"));
    assert!(sanitized.paths.contains_key("literal~name.txt"));
    assert!(sanitized.paths.contains_key("test~1.txt"));
    assert!(sanitize_filenames(&entries(&["package.json", "assets/icon.svg"])).unwrap().is_none());
    assert!(sanitize_filenames(&entries(&["COM0/file"])).unwrap().is_none());
}

#[test]
fn rejects_empty_components_and_collisions() {
    for names in [
        vec!["?/file"],
        vec!["CON/file"],
        vec!["name?.txt", "name.txt"],
        vec!["DIR?/one", "dir/two"],
        vec!["longfilename.txt", "LONG~LMQ?.TXT"],
    ] {
        assert!(sanitize_filenames(&entries(&names)).is_err(), "{names:?}");
    }
}

#[test]
fn component_sanitization_matches_pnpm_11() {
    assert_eq!(sanitize_component("COM0.txt"), "");
    assert_eq!(sanitize_component("name\u{80}?.txt"), "name.txt");
    assert_eq!(sanitize_component(&"\u{e9}".repeat(128)), "\u{e9}".repeat(127));
}
