//! A checkbox prompt shaped like `@inquirer/checkbox`, the prompt behind
//! pnpm's interactive commands, with its keys and vim bindings.
//!
//! The list can hold separators — a group heading, a table's column
//! header — that are drawn but never selected: the cursor skips them and
//! they take no part in the answer.

use console::{Key, Term, measure_text_width};
use owo_colors::{OwoColorize, Stream};
use std::io;

/// One line of a [`CheckboxPrompt`].
pub(crate) enum CheckboxItem<Value> {
    /// Drawn as is, skipped by the cursor.
    Separator(String),
    Choice(CheckboxChoice<Value>),
}

pub(crate) struct CheckboxChoice<Value> {
    /// The line shown while choosing.
    pub name: String,
    /// How the confirmed answer names the choice.
    pub short: String,
    pub value: Value,
}

/// The glyphs and highlighting of a [`CheckboxPrompt`].
///
/// The default is `@inquirer/checkbox`'s own theme: a filled circle for a
/// checked choice, a hollow one otherwise, and the active row in cyan.
pub(crate) struct CheckboxTheme {
    pub checked: String,
    pub unchecked: String,
    pub highlight_active: bool,
}

impl Default for CheckboxTheme {
    fn default() -> Self {
        Self {
            checked: stdout_styled("◉", |text| text.green().to_string()),
            unchecked: "◯".to_string(),
            highlight_active: true,
        }
    }
}

/// What the answer to a [`CheckboxPrompt`] was.
#[derive(Debug)]
pub(crate) enum CheckboxAnswer<Value> {
    /// Enter confirmed these choices, in list order.
    Selected(Vec<Value>),
    /// Ctrl-C.
    Cancelled,
}

pub(crate) struct CheckboxPrompt<Value> {
    message: String,
    items: Vec<CheckboxItem<Value>>,
    checked: Vec<bool>,
    active: usize,
    /// The first item on screen.
    top: usize,
    /// Lines of the list shown at once; `0` until the terminal is measured,
    /// which shows the whole list.
    page_size: usize,
    required: bool,
    theme: CheckboxTheme,
    error: Option<&'static str>,
}

/// What a key did, beyond changing the screen.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyOutcome {
    Redraw,
    Submit,
    Cancel,
}

/// The [`CheckboxPrompt::page_size`] `@inquirer/checkbox` is given when the
/// terminal height is unknown, and the least it is given otherwise.
const MIN_PAGE_SIZE: usize = 7;
/// The lines of a frame that are not the list: the message and the
/// help line, with room for a wrapped message and the error line.
const FRAME_OVERHEAD: usize = 6;

impl<Value> CheckboxPrompt<Value> {
    pub(crate) fn new(message: impl Into<String>, items: Vec<CheckboxItem<Value>>) -> Self {
        let checked = vec![false; items.len()];
        let active = items.iter().position(is_choice).unwrap_or_default();
        Self {
            message: message.into(),
            items,
            checked,
            active,
            top: 0,
            page_size: 0,
            required: false,
            theme: CheckboxTheme::default(),
            error: None,
        }
    }

    /// Refuse to confirm an empty selection.
    pub(crate) fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub(crate) fn theme(mut self, theme: CheckboxTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Run the prompt on the terminal behind stdout, where pnpm draws it,
    /// or behind stderr when stdout is redirected.
    ///
    /// Fails when neither is a terminal or the list has no choice to make.
    pub(crate) fn interact(mut self) -> io::Result<CheckboxAnswer<Value>> {
        if !self.items.iter().any(is_choice) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the checkbox prompt was given no choice to make",
            ));
        }
        let term =
            [Term::stdout(), Term::stderr()].into_iter().find(Term::is_term).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "the checkbox prompt needs a terminal on stdout or stderr",
                )
            })?;
        if self.page_size == 0 {
            self.page_size = page_size_for(&term);
        }
        term.hide_cursor()?;
        let answer = self.interact_on(&term);
        term.show_cursor()?;
        term.flush()?;
        answer
    }

    fn interact_on(&mut self, term: &Term) -> io::Result<CheckboxAnswer<Value>> {
        let columns = usize::from(term.size().1);
        let mut drawn_rows = 0;
        loop {
            let frame = self.render_frame();
            term.clear_last_lines(drawn_rows)?;
            term.write_line(&frame)?;
            term.flush()?;
            drawn_rows = terminal_rows(&frame, columns);
            match self.handle_key(&term.read_key_raw()?) {
                KeyOutcome::Redraw => {}
                KeyOutcome::Submit => {
                    term.clear_last_lines(drawn_rows)?;
                    term.write_line(&self.render_answer())?;
                    return Ok(CheckboxAnswer::Selected(self.take_selected()));
                }
                KeyOutcome::Cancel => return Ok(CheckboxAnswer::Cancelled),
            }
        }
    }

    pub(crate) fn handle_key(&mut self, key: &Key) -> KeyOutcome {
        // A refused Enter is answered by whatever the user does next.
        if *key != Key::Enter {
            self.error = None;
        }
        match key {
            Key::Enter => {
                if self.required && !self.checked.iter().any(|&checked| checked) {
                    self.error = Some("At least one choice must be selected");
                    return KeyOutcome::Redraw;
                }
                return KeyOutcome::Submit;
            }
            Key::CtrlC => return KeyOutcome::Cancel,
            Key::ArrowUp | Key::Char('k') => self.move_active(-1),
            Key::ArrowDown | Key::Char('j') => self.move_active(1),
            Key::Char(' ') => self.checked[self.active] = !self.checked[self.active],
            Key::Char('a') => {
                let select_all = self
                    .items
                    .iter()
                    .zip(&self.checked)
                    .any(|(item, &checked)| is_choice(item) && !checked);
                self.check_every_choice(|_| select_all);
            }
            Key::Char('i') => self.check_every_choice(|checked| !checked),
            Key::Char(digit @ '1'..='9') => {
                let nth = usize::from(*digit as u8 - b'1');
                if let Some(index) = self
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| is_choice(item))
                    .nth(nth)
                    .map(|(index, _)| index)
                {
                    self.active = index;
                    self.checked[index] = !self.checked[index];
                }
            }
            _ => {}
        }
        self.scroll_into_view();
        KeyOutcome::Redraw
    }

    /// Move the cursor `offset` choices, wrapping around the list and
    /// stepping over separators.
    fn move_active(&mut self, offset: isize) {
        let len = self.items.len();
        let mut next = self.active;
        loop {
            next = (next as isize + offset).rem_euclid(len as isize) as usize;
            if is_choice(&self.items[next]) {
                break;
            }
        }
        self.active = next;
    }

    fn check_every_choice(&mut self, checked: impl Fn(bool) -> bool) {
        for (item, slot) in self.items.iter().zip(&mut self.checked) {
            if is_choice(item) {
                *slot = checked(*slot);
            }
        }
    }

    /// Keep the active row on the page, and with it the separators
    /// directly above it — a group's heading and column header — when
    /// they fit.
    fn scroll_into_view(&mut self) {
        if self.page_size == 0 {
            return;
        }
        if self.active < self.top {
            self.top = self.active;
        } else if self.active >= self.top + self.page_size {
            self.top = self.active + 1 - self.page_size;
        }
        while self.top > 0
            && !is_choice(&self.items[self.top - 1])
            && self.active + 1 - (self.top - 1) <= self.page_size
        {
            self.top -= 1;
        }
    }

    /// The screen while choosing: the message, the visible page of the
    /// list, an error if the last Enter was refused, and the key help.
    pub(crate) fn render_frame(&self) -> String {
        let prefix = stdout_styled("?", |text| text.blue().to_string());
        let message = stdout_styled(&self.message, |text| text.bold().to_string());
        let mut lines = vec![format!("{prefix} {message}")];
        lines.extend(self.render_page());
        lines.push(" ".to_string());
        if let Some(error) = self.error {
            let error = format!("> {error}");
            lines.push(stdout_styled(&error, |text| text.red().to_string()));
        }
        lines.push(render_help_line());
        lines.join("\n").trim_end().to_string()
    }

    fn render_page(&self) -> Vec<String> {
        let end = if self.page_size == 0 {
            self.items.len()
        } else {
            (self.top + self.page_size).min(self.items.len())
        };
        (self.top..end).map(|index| self.render_item(index)).collect()
    }

    fn render_item(&self, index: usize) -> String {
        match &self.items[index] {
            CheckboxItem::Separator(text) => format!(" {text}"),
            CheckboxItem::Choice(choice) => {
                let active = index == self.active;
                let cursor = if active { "❯" } else { " " };
                let checkbox =
                    if self.checked[index] { &self.theme.checked } else { &self.theme.unchecked };
                let line = format!("{cursor}{checkbox} {}", choice.name);
                if active && self.theme.highlight_active {
                    stdout_styled(&line, |text| text.cyan().to_string())
                } else {
                    line
                }
            }
        }
    }

    /// The line left behind once Enter confirmed the selection.
    pub(crate) fn render_answer(&self) -> String {
        let prefix = stdout_styled("✔", |text| text.green().to_string());
        let message = stdout_styled(&self.message, |text| text.bold().to_string());
        let shorts = self
            .selected_choices()
            .map(|choice| choice.short.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let answer = stdout_styled(&shorts, |text| text.cyan().to_string());
        format!("{prefix} {message} {answer}")
    }

    pub(crate) fn selected_choices(&self) -> impl Iterator<Item = &CheckboxChoice<Value>> {
        self.items.iter().zip(&self.checked).filter_map(|(item, &checked)| match item {
            CheckboxItem::Choice(choice) if checked => Some(choice),
            _ => None,
        })
    }

    fn take_selected(&mut self) -> Vec<Value> {
        let checked = std::mem::take(&mut self.checked);
        std::mem::take(&mut self.items)
            .into_iter()
            .zip(checked)
            .filter_map(|(item, checked)| match item {
                CheckboxItem::Choice(choice) if checked => Some(choice.value),
                _ => None,
            })
            .collect()
    }
}

fn is_choice<Value>(item: &CheckboxItem<Value>) -> bool {
    matches!(item, CheckboxItem::Choice(_))
}

/// `↑↓ navigate • space select • a all • i invert • ⏎ submit`, with the
/// keys in bold and the rest dimmed.
fn render_help_line() -> String {
    [("↑↓", "navigate"), ("space", "select"), ("a", "all"), ("i", "invert"), ("⏎", "submit")]
        .into_iter()
        .map(|(key, action)| {
            let key = stdout_styled(key, |text| text.bold().to_string());
            let action = stdout_styled(action, |text| text.dimmed().to_string());
            format!("{key} {action}")
        })
        .collect::<Vec<_>>()
        .join(&stdout_styled(" • ", |text| text.dimmed().to_string()))
}

/// pnpm's `interactivePromptPageSize()`: the terminal height less the
/// frame around the list, and never fewer than seven lines.
fn page_size_for(term: &Term) -> usize {
    term.size_checked().map_or(MIN_PAGE_SIZE, |(rows, _)| {
        usize::from(rows).saturating_sub(FRAME_OVERHEAD).max(MIN_PAGE_SIZE)
    })
}

/// The terminal rows `frame` occupies once lines wider than the terminal
/// wrap, which is how many rows clearing it has to reach back over.
fn terminal_rows(frame: &str, columns: usize) -> usize {
    frame
        .split('\n')
        .map(|line| {
            if columns == 0 {
                return 1;
            }
            measure_text_width(line).div_ceil(columns).max(1)
        })
        .sum()
}

fn stdout_styled(text: &str, style: impl Fn(&str) -> String) -> String {
    text.if_supports_color(Stream::Stdout, |text| style(text)).to_string()
}

#[cfg(test)]
mod tests;
