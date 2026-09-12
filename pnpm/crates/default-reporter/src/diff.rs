//! Differential frame renderer — ports the `ansi-diff` algorithm to compute
//! the minimal ANSI escape sequence that transforms the previously rendered
//! frame into the new one.
//!
//! Unchanged lines are skipped entirely, so sticky blocks (lockfile verdicts,
//! deprecation warnings) are not re-written on every progress tick. This is
//! the same algorithm used by the TypeScript `@pnpm/cli.default-reporter`
//! via the npm `ansi-diff` package.

use std::fmt::Write as _;

use crate::format::visible_width;

// `visible_width` counts one column per char (matching `string-length` in the
// TS reporter), not `wcwidth` (which counts CJK/wide chars as 2). This
// matches the TS `ansi-diff` usage where `string-length` is the width source.
// ASCII-dominant progress output is unaffected; CJK package names would be
// undercounted, same as the pre-pnpm/pnpm#12351 behavior.

/// Renders the differential between successive frames.
pub struct Diff {
    col: usize,
    row: usize,
    width: usize,
    lines: Vec<Line>,
}

impl Diff {
    /// A differ for frames `width` columns wide. Every frame handed to
    /// [`Self::update_into`] is wrapped at this width, so it has to be the
    /// terminal's real column count: a differ narrower than the terminal
    /// computes cursor moves for wraps that never happened.
    #[must_use]
    pub fn new(width: usize) -> Self {
        Diff { col: 0, row: 0, width, lines: Vec::new() }
    }

    /// Forget the previous frame and the tracked cursor position, so the
    /// next [`Self::update_into`] emits a full redraw. Call it whenever
    /// something other than this differ wrote to the terminal — an
    /// interactive prompt, a spawned process — and its own idea of where
    /// the cursor is no longer holds.
    pub fn reset(&mut self) {
        self.col = 0;
        self.row = 0;
        self.lines.clear();
    }

    /// Appends to `out` the ANSI escape sequence that transforms the previous
    /// frame into `frame`. The caller wraps this with `\r` (column reset) and
    /// `\x1b[0J` (erase below frame), composing the whole redraw into one
    /// buffer so it reaches the terminal as a single write.
    pub fn update_into(&mut self, frame: &str, out: &mut String) {
        let next = Line::split(frame, self.width);
        let min = next.len().min(self.lines.len());

        // Take ownership of the previous lines so the borrow of `self` ends
        // and we can freely mutate `self.col` / `self.row` during the loop.
        let old = std::mem::take(&mut self.lines);

        let scrub = self.rewrite_changed(&next[..min], &old, out);
        self.append_lines(&next[min..], scrub, out);
        self.clear_trailing(&next, &old, out);

        if let Some(last) = next.last() {
            self.move_to(out, last.remainder, last.row + last.height);
        }

        self.lines = next;
    }

    /// Rewrite the lines that changed, and report whether any of them moved
    /// or changed height: from the first such line on, every rewrite also has
    /// to erase what the previous frame left to the right of it.
    fn rewrite_changed(&mut self, next: &[Line], old: &[Line], out: &mut String) -> bool {
        let mut scrub = false;
        for (index, new_line) in next.iter().enumerate() {
            let old_line = &old[index];
            if new_line.same_text_at(old_line) {
                continue;
            }
            if !scrub
                && self.col != self.width
                && new_line.try_inline_diff(old_line, out, &mut self.col, &mut self.row, self.width)
            {
                continue;
            }
            self.move_to(out, 0, new_line.row);
            out.push_str(&new_line.raw);
            scrub |= new_line.moved_from(old_line);
            if scrub || old_line.length > new_line.length {
                out.push_str("\x1b[0K");
            }
            self.advance_past(new_line, out);
        }
        scrub
    }

    /// Draw the lines the previous frame did not have.
    fn append_lines(&mut self, lines: &[Line], scrub: bool, out: &mut String) {
        for new_line in lines {
            self.move_to(out, 0, new_line.row);
            out.push_str(&new_line.raw);
            if scrub {
                out.push_str("\x1b[0K");
            }
            self.advance_past(new_line, out);
        }
    }

    /// Erase whatever the previous frame drew below the new one.
    fn clear_trailing(&mut self, next: &[Line], old: &[Line], out: &mut String) {
        let Some(old_last) = old.last() else { return };
        let old_last_row = old_last.row + old_last.height;
        let new_last_row = next.last().map_or(0, |line| line.row + line.height);
        if next.is_empty() || new_last_row < old_last_row {
            self.clear_down(out, old_last_row);
        }
    }

    /// Emit the line's trailing newline, if it has one, and record where the
    /// cursor now sits.
    fn advance_past(&mut self, line: &Line, out: &mut String) {
        if line.newline {
            out.push('\n');
            self.col = 0;
            self.row = line.row + line.height + 1;
        } else {
            self.col = line.remainder;
            self.row = line.row + line.height;
        }
    }

    #[cfg(test)]
    pub fn update(&mut self, frame: &str) -> String {
        let mut out = String::new();
        self.update_into(frame, &mut out);
        out
    }

    fn move_to(&mut self, out: &mut String, col: usize, row: usize) {
        move_to(out, &mut self.col, &mut self.row, col, row);
    }

    /// Clear lines from the cursor's current position down to `target_row`,
    /// matching `ansi-diff`'s `_clearDown`. The first line starts at the
    /// cursor's current column; subsequent lines start at column 0.
    fn clear_down(&mut self, out: &mut String, target_row: usize) {
        let mut col = self.col;
        for current_row in self.row..=target_row {
            self.move_to(out, col, current_row);
            out.push_str("\x1b[0K");
            col = 0;
        }
    }
}

#[derive(Clone)]
struct Line {
    raw: String,
    row: usize,
    length: usize,
    height: usize,
    remainder: usize,
    newline: bool,
}

impl Line {
    fn new(text: &str, row: usize, newline: bool, width: usize) -> Self {
        let length = visible_width(text);
        let (height, remainder) = match width {
            0 => (0, length),
            term_width => {
                let line_height = length / term_width;
                let line_remainder = length % term_width;
                if line_height > 0 && line_remainder == 0 {
                    (line_height - 1, term_width)
                } else {
                    (line_height, line_remainder)
                }
            }
        };
        Line { raw: text.to_string(), row, length, height, remainder, newline }
    }

    fn split(input: &str, width: usize) -> Vec<Self> {
        let parts: Vec<&str> = input.split('\n').collect();
        let count = parts.len();
        let mut row_offset = 0;
        parts
            .into_iter()
            .enumerate()
            .map(|(idx, text)| {
                let newline = idx < count - 1;
                let line = Line::new(text, row_offset, newline, width);
                row_offset += line.height + u8::from(newline) as usize;
                line
            })
            .collect()
    }

    /// Inline diff: if only a few characters changed, write just those
    /// instead of the whole line. Only attempted on lines without ANSI
    /// escape codes (plain text like progress lines).
    /// Whether this line renders exactly as `other` did at the same row.
    fn same_text_at(&self, other: &Line) -> bool {
        self.raw == other.raw && self.row == other.row && self.newline == other.newline
    }

    /// Whether the line sits at a different row, or occupies a different
    /// number of them, than `other` did.
    fn moved_from(&self, other: &Line) -> bool {
        self.row != other.row || self.height != other.height
    }

    fn try_inline_diff(
        &self,
        other: &Self,
        out: &mut String,
        col: &mut usize,
        row: &mut usize,
        width: usize,
    ) -> bool {
        if self.length != other.length
            || self.row != other.row
            || !self.newline
            || !other.newline
            || self.raw.contains('\u{1b}')
            || other.raw.contains('\u{1b}')
        {
            return false;
        }
        let self_chars: Vec<char> = self.raw.chars().collect();
        let other_chars: Vec<char> = other.raw.chars().collect();
        let left = self_chars.iter().zip(&other_chars).take_while(|(ca, cb)| ca == cb).count();
        let right = self_chars
            .iter()
            .rev()
            .zip(other_chars.iter().rev())
            .take_while(|(ca, cb)| ca == cb)
            .count();
        let changed_len = self_chars.len().saturating_sub(left + right);
        if left + right <= 4 || left + changed_len >= width.saturating_sub(1) {
            return false;
        }
        move_to(out, col, row, left, self.row);
        let changed: String = self_chars[left..left + changed_len].iter().collect();
        out.push_str(&changed);
        *col = left + changed_len;
        true
    }
}

fn move_to(
    out: &mut String,
    col: &mut usize,
    row: &mut usize,
    target_col: usize,
    target_row: usize,
) {
    if target_col > *col {
        let _ = write!(out, "\x1b[{}C", target_col - *col);
    } else if target_col < *col {
        let _ = write!(out, "\x1b[{}D", *col - target_col);
    }
    if target_row > *row {
        let _ = write!(out, "\x1b[{}B", target_row - *row);
    } else if target_row < *row {
        let _ = write!(out, "\x1b[{}A", *row - target_row);
    }
    *col = target_col;
    *row = target_row;
}

#[cfg(test)]
mod tests;
