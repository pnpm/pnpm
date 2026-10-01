use super::{CheckboxPrompt, CheckboxTheme, Key, KeyOutcome};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Checkbox,
    MultiSelect,
    Select,
}

impl<Value> CheckboxPrompt<Value> {
    pub(crate) fn dialoguer_defaults(mut self, defaults: Vec<bool>) -> Self {
        assert_eq!(self.checked.len(), defaults.len());
        self.checked = defaults;
        self.mode = Mode::MultiSelect;
        self
    }

    pub(crate) fn single_selection(mut self, active: usize) -> Self {
        self.viewport.active = active;
        self.checked.fill(false);
        self.checked[active] = true;
        self.mode = Mode::Select;
        self.theme = CheckboxTheme {
            checked: String::new(),
            unchecked: String::new(),
            highlight_active: true,
        };
        self
    }

    pub(super) fn render_host_help_line(&self) -> String {
        if self.mode == Mode::Select {
            "↑/↓ Move • Space/Enter Select • Esc Cancel".into()
        } else {
            super::render_help_line()
        }
    }

    pub(super) fn handle_dialoguer_key(&mut self, key: &Key) -> Option<KeyOutcome> {
        if self.mode == Mode::Checkbox {
            return None;
        }
        if matches!(key, Key::Escape | Key::Char('q') | Key::CtrlC) {
            return Some(KeyOutcome::Cancel);
        }
        if self.mode == Mode::Select && matches!(key, Key::Enter | Key::Char(' ')) {
            return Some(KeyOutcome::Submit);
        }
        if self.move_for_dialoguer_key(key) {
            if self.mode == Mode::Select {
                self.checked.fill(false);
                self.checked[self.viewport.active] = true;
            }
            self.scroll_into_view();
            return Some(KeyOutcome::Redraw);
        }
        (self.mode == Mode::Select).then_some(KeyOutcome::Redraw)
    }

    fn move_for_dialoguer_key(&mut self, key: &Key) -> bool {
        let offset = match key {
            Key::ArrowUp | Key::BackTab | Key::Char('k') => -1,
            Key::ArrowDown | Key::Tab | Key::Char('j') => 1,
            Key::ArrowLeft | Key::Char('h') => -(self.viewport.page_size as isize),
            Key::ArrowRight | Key::Char('l') => self.viewport.page_size as isize,
            _ => return false,
        };
        self.move_active(offset);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
