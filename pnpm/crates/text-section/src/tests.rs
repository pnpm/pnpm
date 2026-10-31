use super::{Section, find_sections, wrap_section};

#[test]
fn finds_every_closed_block() {
    let content = "a\n# pnpm\none\n# pnpm end\nb\n# pnpm\n# pnpm\ntwo\r\n# pnpm end\r\n# pnpm\n";

    let sections = find_sections(content, "pnpm");

    assert_eq!(
        sections,
        [
            Section { range: 2..23, body: "one".to_string() },
            Section { range: 33..55, body: "two".to_string() },
        ],
    );
    assert_eq!(&content[sections[1].range.clone()], "# pnpm\ntwo\r\n# pnpm end");
}

#[test]
fn wraps_a_body_in_markers() {
    assert_eq!(wrap_section("pnpm", "/auth"), "# pnpm\n/auth\n# pnpm end");
}
