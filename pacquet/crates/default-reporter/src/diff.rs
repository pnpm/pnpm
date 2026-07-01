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

/// Renders the differential between successive frames.
pub(crate) struct Diff {
    col: usize,
    row: usize,
    width: usize,
    lines: Vec<Line>,
}

impl Diff {
    pub(crate) fn new(width: usize) -> Self {
        Diff { col: 0, row: 0, width, lines: Vec::new() }
    }

    /// Returns the ANSI escape sequence that transforms the previous frame
    /// into `frame`. The caller wraps this with `\r` (column reset) and
    /// `\x1b[0J` (erase below frame).
    pub(crate) fn update(&mut self, frame: &str) -> String {
        let next = Line::split(frame, self.width);
        let mut out = String::new();
        let min = next.len().min(self.lines.len());
        let mut scrub = false;

        // Clone the previous lines so the borrow of `self.lines` ends before
        // we mutate `self.col` / `self.row`.
        let old: Vec<Line> = self.lines.iter().take(min).cloned().collect();

        for (idx, new_line) in next.iter().enumerate().take(min) {
            let old_line = &old[idx];
            if new_line.raw == old_line.raw
                && new_line.row == old_line.row
                && new_line.newline == old_line.newline
            {
                continue;
            }
            let old_len = old_line.length;
            let old_row = old_line.row;
            let old_height = old_line.height;
            if !scrub
                && new_line.try_inline_diff(
                    old_line,
                    &mut out,
                    &mut self.col,
                    &mut self.row,
                    self.width,
                )
            {
                continue;
            }
            self.move_to(&mut out, 0, new_line.row);
            out.push_str(&new_line.raw);
            if new_line.row != old_row || new_line.height != old_height {
                scrub = true;
            }
            if old_len > new_line.length || scrub {
                out.push_str("\x1b[0K");
            }
            if new_line.newline {
                out.push('\n');
                self.col = 0;
                self.row = new_line.row + new_line.height + 1;
            } else {
                self.col = new_line.remainder;
                self.row = new_line.row + new_line.height;
            }
        }

        for new_line in &next[min..] {
            self.move_to(&mut out, 0, new_line.row);
            out.push_str(&new_line.raw);
            if scrub {
                out.push_str("\x1b[0K");
            }
            if new_line.newline {
                out.push('\n');
                self.col = 0;
                self.row = new_line.row + new_line.height + 1;
            } else {
                self.col = new_line.remainder;
                self.row = new_line.row + new_line.height;
            }
        }

        if let Some(old_last) = self.lines.last() {
            let new_last_row = next.last().map_or(0, |line| line.row + line.height);
            let old_last_row = old_last.row + old_last.height;
            if new_last_row < old_last_row {
                self.move_to(&mut out, 0, new_last_row);
                for _ in new_last_row..old_last_row {
                    out.push_str("\x1b[0K\n");
                    self.row += 1;
                }
            }
        }

        if let Some(last) = next.last() {
            self.move_to(&mut out, last.remainder, last.row + last.height);
        }

        self.lines = next;
        out
    }

    fn move_to(&mut self, out: &mut String, col: usize, row: usize) {
        if col > self.col {
            let _ = write!(out, "\x1b[{}C", col - self.col);
        } else if col < self.col {
            let _ = write!(out, "\x1b[{}D", self.col - col);
        }
        if row > self.row {
            let _ = write!(out, "\x1b[{}B", row - self.row);
        } else if row < self.row {
            let _ = write!(out, "\x1b[{}A", self.row - row);
        }
        self.col = col;
        self.row = row;
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
