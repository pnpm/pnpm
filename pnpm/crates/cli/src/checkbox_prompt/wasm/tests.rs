use super::{CheckboxPrompt, Key, KeyOutcome};
use crate::checkbox_prompt::{CheckboxChoice, CheckboxItem};

fn prompt() -> CheckboxPrompt<usize> {
    CheckboxPrompt::new(
        "Packages",
        (0..3)
            .map(|value| {
                CheckboxItem::Choice(CheckboxChoice {
                    name: value.to_string(),
                    short: value.to_string(),
                    value,
                })
            })
            .collect(),
    )
}

fn selected(prompt: &CheckboxPrompt<usize>) -> Vec<usize> {
    prompt
        .selected_choices()
        .map(|choice| choice.value)
        .collect()
}

#[test]
fn multiselect_preserves_defaults_and_tab_navigation() {
    let mut prompt = prompt().dialoguer_defaults(vec![false, true, false]);
    assert_eq!(selected(&prompt), [1]);
    prompt.handle_key(&Key::Tab);
    prompt.handle_key(&Key::Char(' '));
    assert!(selected(&prompt).is_empty());
    prompt.handle_key(&Key::BackTab);
    prompt.handle_key(&Key::Char(' '));
    assert_eq!(selected(&prompt), [0]);
    assert_eq!(prompt.handle_key(&Key::Escape), KeyOutcome::Cancel);
}

#[test]
fn single_select_confirms_only_the_active_item() {
    let mut prompt = prompt().single_selection(0);
    prompt.handle_key(&Key::ArrowDown);
    prompt.handle_key(&Key::Char('a'));
    assert_eq!(selected(&prompt), [1]);
    assert_eq!(prompt.handle_key(&Key::Char(' ')), KeyOutcome::Submit);
    assert!(!prompt.render_host_help_line().contains("all"));
}

#[test]
fn dialoguer_cancellation_does_not_submit_defaults() {
    for key in [Key::CtrlC, Key::Escape, Key::Char('q')] {
        let mut prompt = prompt().single_selection(1);
        assert_eq!(prompt.handle_key(&key), KeyOutcome::Cancel);
    }
}
