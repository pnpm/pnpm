use super::split_allow_build_selectors;
use pretty_assertions::assert_eq;

fn split(values: &[&str]) -> Vec<String> {
    let values: Vec<String> = values
        .iter()
        .map(ToString::to_string)
        .collect();
    split_allow_build_selectors(&values)
}

#[test]
fn splits_comma_separated_package_selectors() {
    assert_eq!(split(&["esbuild,sharp"]), ["esbuild", "sharp"]);
    assert_eq!(split(&["esbuild@0.27.7,sharp@0.34.5"]), ["esbuild@0.27.7", "sharp@0.34.5"]);
    assert_eq!(split(&["esbuild@0.27.7, sharp@0.34.5"]), ["esbuild@0.27.7", "sharp@0.34.5"]);
    assert_eq!(split(&["@swc/core,esbuild,!core-js"]), ["@swc/core", "esbuild", "!core-js"]);
    assert_eq!(split(&["esbuild,sharp", "fsevents"]), ["esbuild", "sharp", "fsevents"]);
    assert_eq!(split(&["nx@21.6.4 || 21.6.5,esbuild"]), ["nx@21.6.4 || 21.6.5", "esbuild"]);
    assert_eq!(split(&["esbuild@latest,sharp"]), ["esbuild@latest", "sharp"]);
}

#[test]
fn keeps_empty_items_so_they_are_rejected() {
    assert_eq!(split(&["esbuild,"]), ["esbuild", ""]);
    assert_eq!(split(&[",esbuild"]), ["", "esbuild"]);
}

#[test]
fn keeps_selectors_with_a_colon_whole() {
    for selector in [
        "pkg@https://example.com/a,b.tgz",
        "!pkg@https://example.com/a,b.tgz",
        "https://example.com/a,b.tgz",
        "pkg@file:./dir,with,comma",
        "pkg@link:../a,b",
        "pkg@git+ssh://git@github.com/a/b.git#a,b",
        "pkg@npm:other@1,sharp",
        r"C:\a,b",
        "esbuild,local@file:./packages/local",
    ] {
        assert_eq!(split(&[selector]), [selector], "{selector}");
    }
}

#[test]
fn trims_a_selector_with_a_colon_but_keeps_its_commas() {
    assert_eq!(split(&[" pkg@https://example.com/a,b.tgz "]), ["pkg@https://example.com/a,b.tgz"],);
}
