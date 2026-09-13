use super::{
    FORCE_APPEND_ONLY, PromptBuffer, Sink, TerminalViewport, colors_enabled, cwd, diff,
    frame_offset_of_line, is_stderr_output, rendered_rows, reporter_options, terminal_size,
};
use crate::{
    colors::Colors,
    state::{Output, ReporterState},
};
use pnpm_reporter::PromptAction;
use std::{
    io::{IsTerminal, Write},
    time::{Duration, Instant},
};

impl Sink {
    pub(super) fn new() -> Self {
        let is_tty = output_is_terminal();
        let append_only = !is_tty
            || FORCE_APPEND_ONLY
                .get()
                .copied()
                .unwrap_or(false);
        let (columns, rows) = if is_tty {
            terminal_size().unwrap_or((80, None))
        } else {
            (80, None)
        };
        // pnpm's `outputMaxWidth`: `columns - 2` on a TTY, else 80.
        let width = if is_tty {
            columns.saturating_sub(2)
        } else {
            80
        };
        let colors = Colors {
            enabled: colors_enabled(is_tty),
        };
        let state =
            ReporterState::new_with_options(cwd(), width, colors, reporter_options(append_only));
        let diff = diff::Diff::new(columns);
        let throttle = if append_only {
            Duration::from_secs(1)
        } else {
            Duration::from_millis(200)
        };
        Sink {
            state,
            diff,
            frame_buf: String::new(),
            throttle,
            last_write: None,
            viewport: TerminalViewport::new(columns, rows),
            prompt: PromptBuffer::default(),
        }
    }

    pub(super) fn on_prompt(&mut self, action: PromptAction) {
        if is_stderr_output() {
            let mut out = std::io::stderr().lock();
            self.on_prompt_to(action, &mut out);
        } else {
            let mut out = std::io::stdout().lock();
            self.on_prompt_to(action, &mut out);
        }
    }

    pub(super) fn on_prompt_to(&mut self, action: PromptAction, out: &mut impl Write) {
        match action {
            PromptAction::Start => {
                self.prompt.active = true;
                self.prompt.lines.clear();
                self.prompt.frame = None;
            }
            PromptAction::End => {
                self.prompt.active = false;
                self.diff.reset();
                self.last_write = None;
                let mut wrote = false;
                if !self.prompt.lines.is_empty() {
                    let lines = std::mem::take(&mut self.prompt.lines);
                    wrote |= self.write_output(Output::Lines(lines), out);
                }
                if let Some(frame) = self.prompt.frame.take() {
                    wrote |= self.write_output(Output::Frame(frame), out);
                }
                if wrote {
                    self.last_write = Some(Instant::now());
                }
            }
        }
    }

    pub(super) fn write(&mut self, output: Output, coalesceable: bool) {
        if is_stderr_output() {
            let mut out = std::io::stderr().lock();
            self.write_to(output, coalesceable, &mut out);
        } else {
            let mut out = std::io::stdout().lock();
            self.write_to(output, coalesceable, &mut out);
        }
    }

    pub(super) fn write_to(&mut self, output: Output, coalesceable: bool, out: &mut impl Write) {
        if self.prompt.active {
            self.prompt.push(output);
            return;
        }
        // Drop a high-volume progress redraw if the throttle window hasn't
        // elapsed. State is already folded, so the next non-coalesceable
        // event (stats, summary, importing-done, the footer) renders the
        // latest counts.
        if coalesceable && self.last_write.is_some_and(|last| last.elapsed() < self.throttle) {
            return;
        }
        let wrote = self.write_output(output, out);
        if wrote {
            self.last_write = Some(Instant::now());
        }
    }

    /// Returns whether anything was written.
    pub(super) fn write_output(&mut self, output: Output, out: &mut impl Write) -> bool {
        match output {
            Output::None => return false,
            Output::Lines(lines) => {
                for line in lines {
                    let _ = writeln!(out, "{line}");
                }
            }
            Output::Frame(mut frame) => {
                // A trailing newline keeps an interactive prompt on a fresh line
                // below the frame rather than joined onto its last line, and it
                // leaves the differ's tracked cursor at column 0 so it stays in
                // sync with the `\r` prepended on the next update (otherwise the
                // inline diff computes relative moves from a stale column).
                if !frame.ends_with('\n') {
                    frame.push('\n');
                }
                let lines: Vec<&str> = frame[..frame.len() - 1].split('\n').collect();
                self.refresh_terminal_size();
                // `\r` resets the column in case an external process left the
                // cursor mid-line; `\x1b[K` erases trailing characters on the
                // current line; `\x1b[0J` erases anything written below the
                // rendered frame.
                self.frame_buf.clear();
                self.frame_buf.push('\r');
                self.commit_overflow(&lines);
                let visible =
                    &frame[frame_offset_of_line(&frame, &lines, self.viewport.committed_lines)..];
                self.diff.update_into(visible, &mut self.frame_buf);
                self.frame_buf.push_str("\x1b[K\x1b[0J");
                let _ = out.write_all(self.frame_buf.as_bytes());
            }
        }
        let _ = out.flush();
        true
    }

    /// Pick up a window resize, so the frame is fitted to the terminal it is
    /// about to be drawn on rather than the one the process started in.
    pub(super) fn refresh_terminal_size(&mut self) {
        let Some((columns, rows)) = (self.viewport.terminal_size)() else {
            return;
        };
        self.viewport.rows = rows;
        if columns == self.viewport.columns {
            return;
        }
        // The terminal was resized. The frame on screen has reflowed at the new
        // width, so every position the differ tracked against the old one is
        // wrong: start over below what is already there.
        self.viewport.columns = columns;
        self.diff = diff::Diff::new(columns);
    }

    /// Hands the lines of the frame that no longer fit on screen over to the
    /// scrollback, appending the differential that performs the handover to
    /// `frame_buf` and restarting the differ below them.
    ///
    /// The differ redraws by moving the cursor up from the end of its frame, so
    /// it can only reach lines that are still on screen. A frame taller than
    /// the terminal has scrolled its top away, and every later redraw then
    /// stops at the top of the screen — overwriting output above the frame
    /// instead of updating it (pnpm/pnpm#14270). Committing the overflow keeps
    /// the frame within the terminal, at the cost of no longer being able to
    /// revise what was committed.
    pub(super) fn commit_overflow(&mut self, lines: &[&str]) {
        if lines.len() <= self.viewport.committed_lines {
            // The frame no longer reaches past what was committed — an error
            // frame replaces it rather than extending it. Render it whole,
            // below.
            self.viewport.committed_lines = 0;
            self.diff.reset();
            return;
        }
        let Some(rows) = self.viewport.rows else {
            return;
        };
        // One row is left over for the cursor line that the trailing newline
        // puts below the frame.
        let max_rows = rows.saturating_sub(1).max(1);
        let uncommitted_rows: usize = lines[self.viewport.committed_lines..]
            .iter()
            .map(|line| rendered_rows(line, self.viewport.columns))
            .sum();
        let (first_visible, frame_rows) = self.viewport.first_visible_line(lines, max_rows);
        // A frame taller than the terminal has scrolled its own top away —
        // whether because a line outgrew the screen or because the window shrank
        // under it — so no cursor move reaches back into it, and growing the
        // window again does not bring it back. Start afresh below instead,
        // reprinting rather than revising, and leave the commit for the next
        // frame, whose layout is one this differ laid out itself.
        let cannot_revise = self.viewport.rendered_frame_outgrew_terminal
            || self.viewport.rendered_frame_rows > max_rows;
        if cannot_revise || first_visible == self.viewport.committed_lines {
            self.viewport.rendered_frame_rows = uncommitted_rows;
            self.viewport.rendered_frame_outgrew_terminal = uncommitted_rows > max_rows;
            if cannot_revise || self.viewport.rendered_frame_outgrew_terminal {
                self.diff = diff::Diff::new(self.viewport.columns);
            }
            return;
        }
        self.viewport.rendered_frame_rows = frame_rows;
        self.viewport.rendered_frame_outgrew_terminal = false;
        // Shrinking the frame to just the overflow leaves those lines untouched
        // where they already are, erases the rest of the frame below them, and
        // parks the cursor on the next line — where the restarted differ picks
        // up.
        let handover = format!(
            "{}\n",
            lines[self.viewport.committed_lines..first_visible].join("\n"),
        );
        let mut buf = std::mem::take(&mut self.frame_buf);
        self.diff.update_into(&handover, &mut buf);
        self.frame_buf = buf;
        self.diff = diff::Diff::new(self.viewport.columns);
        self.viewport.committed_lines = first_visible;
    }
}

fn output_is_terminal() -> bool {
    if is_stderr_output() {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    }
}
