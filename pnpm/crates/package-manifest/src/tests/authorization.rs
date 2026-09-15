use super::{InitAuthor, assert_eq};

#[test]
fn an_author_renders_every_part_it_was_given() {
    let author = |name, email, url| InitAuthor { name, email, url }.to_string();
    assert_eq!(
        author(Some("pnpm"), Some("xxxxxx@pnpm.com"), Some("https://www.github.com/pnpm")),
        "pnpm <xxxxxx@pnpm.com> (https://www.github.com/pnpm)",
    );
    assert_eq!(author(Some("pnpm"), None, None), "pnpm");
    assert_eq!(author(None, Some("xxxxxx@pnpm.com"), None), " <xxxxxx@pnpm.com>");
    assert_eq!(author(None, None, None), "");
    // A part set to the empty string is a part that was not given.
    assert_eq!(author(Some("pnpm"), Some(""), Some("")), "pnpm");
    assert_eq!(author(Some(""), Some(""), Some("")), "");
}
