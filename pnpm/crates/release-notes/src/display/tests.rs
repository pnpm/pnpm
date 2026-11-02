use super::render_note;
use pretty_assertions::assert_eq;

#[test]
fn shows_link_targets_next_to_their_text() {
    assert_eq!(
        render_note("See [the guide](https://example.com/guide \"Guide\") first.\n"),
        "See the guide (https://example.com/guide) first.",
    );
}

#[test]
fn a_link_whose_text_is_its_target_is_shown_once() {
    assert_eq!(render_note("[https://example.com](https://example.com)"), "https://example.com");
}

#[test]
fn images_are_named_not_fetched() {
    assert_eq!(
        render_note("![diagram](https://example.com/a.png)"),
        "image: diagram (https://example.com/a.png)",
    );
}

#[test]
fn brackets_that_are_not_links_stay() {
    assert_eq!(
        render_note("- [x] done, a[0] and ![not an image"),
        "- [x] done, a[0] and ![not an image",
    );
}

#[test]
fn removes_control_and_bidirectional_override_characters() {
    assert_eq!(render_note("safe\u{1b}[2J\u{202E}text\n\tmore"), "safe[2Jtext\n\tmore");
}
