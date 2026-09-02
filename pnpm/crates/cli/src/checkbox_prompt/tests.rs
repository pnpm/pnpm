use super::{CheckboxChoice, CheckboxItem, CheckboxPrompt, CheckboxTheme, KeyOutcome};
use console::{Key, strip_ansi_codes};

fn separator(text: &str) -> CheckboxItem<&'static str> {
    CheckboxItem::Separator(text.to_string())
}

fn choice(value: &'static str) -> CheckboxItem<&'static str> {
    CheckboxItem::Choice(CheckboxChoice {
        name: format!("{value} 1.0.0 ❯ 2.0.0"),
        short: value.to_string(),
        value,
    })
}

/// Two groups the way `update --interactive` lays them out: a heading and
/// a column header above each group's rows.
fn grouped_prompt() -> CheckboxPrompt<&'static str> {
    CheckboxPrompt::new(
        "Choose",
        vec![
            separator("── dependencies ──"),
            separator("  Package"),
            choice("foo"),
            choice("bar"),
            separator("── devDependencies ──"),
            separator("  Package"),
            choice("baz"),
        ],
    )
}

fn press(prompt: &mut CheckboxPrompt<&'static str>, keys: &[Key]) -> KeyOutcome {
    let mut outcome = KeyOutcome::Redraw;
    for key in keys {
        outcome = prompt.handle_key(key);
    }
    outcome
}

fn selected(prompt: &CheckboxPrompt<&'static str>) -> Vec<&'static str> {
    prompt.selected_choices().map(|choice| choice.value).collect()
}

fn frame_lines(prompt: &CheckboxPrompt<&'static str>) -> Vec<String> {
    strip_ansi_codes(&prompt.render_frame()).lines().map(str::to_string).collect()
}

#[test]
fn the_cursor_starts_on_the_first_choice_below_the_headings() {
    let prompt = grouped_prompt();

    let lines = frame_lines(&prompt);
    assert_eq!(
        &lines[1..5],
        [" ── dependencies ──", "   Package", "❯◯ foo 1.0.0 ❯ 2.0.0", " ◯ bar 1.0.0 ❯ 2.0.0"],
    );
}

#[test]
fn moving_skips_separators_and_wraps_around() {
    let mut prompt = grouped_prompt();

    press(&mut prompt, &[Key::ArrowDown, Key::ArrowDown]);
    assert_eq!(prompt.active, 6, "the second move steps over the devDependencies headings");

    press(&mut prompt, &[Key::ArrowDown]);
    assert_eq!(prompt.active, 2, "moving past the end wraps to the first choice");

    press(&mut prompt, &[Key::Char('k')]);
    assert_eq!(prompt.active, 6, "moving before the start wraps to the last choice");
}

#[test]
fn space_toggles_the_active_choice_only() {
    let mut prompt = grouped_prompt();

    press(&mut prompt, &[Key::Char(' '), Key::Char('j'), Key::Char(' '), Key::Char(' ')]);

    assert_eq!(selected(&prompt), ["foo"]);
}

#[test]
fn a_toggles_every_choice_and_i_inverts() {
    let mut prompt = grouped_prompt();

    press(&mut prompt, &[Key::Char('a')]);
    assert_eq!(selected(&prompt), ["foo", "bar", "baz"]);

    press(&mut prompt, &[Key::Char('a')]);
    assert_eq!(selected(&prompt), Vec::<&str>::new(), "all checked, so `a` clears them");

    press(&mut prompt, &[Key::Char(' '), Key::Char('i')]);
    assert_eq!(selected(&prompt), ["bar", "baz"]);

    press(&mut prompt, &[Key::Char('a')]);
    assert_eq!(selected(&prompt), ["foo", "bar", "baz"], "one unchecked, so `a` checks the rest");
}

#[test]
fn a_digit_toggles_that_choice_counting_choices_only() {
    let mut prompt = grouped_prompt();

    press(&mut prompt, &[Key::Char('3')]);

    assert_eq!(selected(&prompt), ["baz"]);
    assert_eq!(prompt.active, 6);
}

#[test]
fn enter_submits_and_ctrl_c_cancels() {
    let mut prompt = grouped_prompt();

    assert_eq!(press(&mut prompt, &[Key::Char(' '), Key::Enter]), KeyOutcome::Submit);
    assert_eq!(press(&mut prompt, &[Key::CtrlC]), KeyOutcome::Cancel);
}

#[test]
fn a_required_prompt_refuses_an_empty_selection() {
    let mut prompt = grouped_prompt().required(true);

    assert_eq!(press(&mut prompt, &[Key::Enter]), KeyOutcome::Redraw);
    let lines = frame_lines(&prompt);
    assert!(
        lines.contains(&"> At least one choice must be selected".to_string()),
        "no error shown:\n{}",
        lines.join("\n"),
    );

    press(&mut prompt, &[Key::ArrowDown]);
    assert!(!prompt.render_frame().contains("At least one"), "moving clears the error");

    press(&mut prompt, &[Key::Enter, Key::Char('a')]);
    assert!(!prompt.render_frame().contains("At least one"), "toggling all clears the error");
    press(&mut prompt, &[Key::Char('a')]);

    assert_eq!(press(&mut prompt, &[Key::Char(' '), Key::Enter]), KeyOutcome::Submit);
}

#[test]
fn the_frame_ends_with_the_key_help() {
    let prompt = grouped_prompt();

    let lines = frame_lines(&prompt);
    assert_eq!(lines[0], "? Choose");
    assert_eq!(
        lines.last().map(String::as_str),
        Some("↑↓ navigate • space select • a all • i invert • ⏎ submit"),
    );
    assert_eq!(lines[lines.len() - 2].trim(), "", "a blank line separates the list from the help");
}

#[test]
fn the_answer_names_the_selection_by_its_short_form() {
    let mut prompt = grouped_prompt();

    press(&mut prompt, &[Key::Char(' '), Key::Char('3')]);

    assert_eq!(strip_ansi_codes(&prompt.render_answer()), "✔ Choose foo, baz");
}

#[test]
fn the_theme_picks_the_icons() {
    let mut prompt = grouped_prompt().theme(CheckboxTheme {
        checked: "●".to_string(),
        unchecked: "○".to_string(),
        highlight_active: false,
    });

    press(&mut prompt, &[Key::Char(' ')]);

    let lines = frame_lines(&prompt);
    assert_eq!(lines[3], "❯● foo 1.0.0 ❯ 2.0.0");
    assert_eq!(lines[4], " ○ bar 1.0.0 ❯ 2.0.0");
}

/// The page scrolls to keep the cursor on it. When the rows directly
/// above the cursor are separators — a group's heading and column
/// header — they scroll on with it, so a group is never shown without
/// its title.
#[test]
fn the_page_follows_the_cursor_and_keeps_the_headings_above_it() {
    let mut prompt = grouped_prompt();
    prompt.page_size = 3;

    let page = |prompt: &CheckboxPrompt<&'static str>| frame_lines(prompt)[1..4].to_vec();
    assert_eq!(page(&prompt), [" ── dependencies ──", "   Package", "❯◯ foo 1.0.0 ❯ 2.0.0"]);

    press(&mut prompt, &[Key::ArrowDown, Key::ArrowDown]);
    assert_eq!(page(&prompt), [" ── devDependencies ──", "   Package", "❯◯ baz 1.0.0 ❯ 2.0.0"]);

    press(&mut prompt, &[Key::ArrowDown]);
    assert_eq!(
        page(&prompt),
        [" ── dependencies ──", "   Package", "❯◯ foo 1.0.0 ❯ 2.0.0"],
        "wrapping to the first row brings its headings back",
    );

    press(&mut prompt, &[Key::ArrowUp, Key::ArrowUp]);
    assert_eq!(
        page(&prompt),
        ["❯◯ bar 1.0.0 ❯ 2.0.0", " ── devDependencies ──", "   Package"],
        "a row under another row scrolls in on its own",
    );
}

#[test]
fn a_prompt_without_choices_cannot_run() {
    let prompt: CheckboxPrompt<&str> = CheckboxPrompt::new("Choose", vec![separator("── none ──")]);

    let error = prompt.interact().expect_err("no choice to make");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}
