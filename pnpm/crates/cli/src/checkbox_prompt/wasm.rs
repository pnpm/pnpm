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
mod tests;
