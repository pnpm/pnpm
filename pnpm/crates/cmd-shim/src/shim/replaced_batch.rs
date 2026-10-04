//! Replacing a `.cmd` shim that cmd.exe may still be running.
//!
//! cmd.exe reads a batch file one line at a time, by byte offset, and reopens
//! the file for every line. A [`CmdShimBatch::Kept`] shim is still being read
//! while its target runs, so when the target replaces the shim and exits, as
//! `pnpm self-update` does, cmd.exe reads on in the new file from the offset
//! where the old file's target command ended. Whatever it finds there runs:
//! the tail of a line, and the target once more when the new shim's command
//! line lies past that offset (pnpm/pnpm#16573).
//!
//! [`CmdShimBatch::Kept`]: super::CmdShimBatch::Kept

use super::{BATCH_END, CODEPAGE_RESTORE_TRAILER};

/// The line a shim starts with when it jumps over the lines that end the
/// replaced batch.
const JUMP_OVER: &str = "@GOTO :pnpm\r\n";
const JUMP_TARGET: &str = ":pnpm\r\n";
/// Ends the replaced batch when it has no code page to restore, with its
/// target's exit code. `GOTO :EOF` would leave `%ERRORLEVEL%` alone, but
/// `cmd /c` reports the result of the last command it ran, and that `GOTO`
/// succeeds. An `ERRORLEVEL` variable inherited from the caller shadows the
/// exit code in that expansion, and a `SET "ERRORLEVEL="` line before it makes
/// cmd.exe report 0 instead, so that environment is left as it is.
const END_REPLACED_BATCH: &str = "@EXIT /B %ERRORLEVEL%\r\n";

const REM_LINE_MIN: usize = "@REM\r\n".len();
/// Well under cmd.exe's line limit of 8191 characters.
const REM_LINE_MAX: usize = 4000;
/// The text the padding carries, for whoever reads the shim. ASCII with none of
/// cmd.exe's metacharacters, so a `REM` line holds any prefix of it.
const PADDING_NOTE: &str =
    "cmd.exe may still be running the pnpm.cmd this file replaced, and reads on at the next line.";

/// Lay out `shim`, a [`CmdShimBatch::EndedBeforeTarget`] shim, to replace
/// `replaced`, the `.cmd` file at its path.
///
/// The result has the lines that end the replaced batch at the offset where
/// cmd.exe reads on in `replaced`, and a leading `GOTO` that jumps over them
/// when the shim runs from the start. `shim` is returned as is when `replaced`
/// ends its batch before its target, since nothing reads on in it, and when
/// that offset leaves no room for the jump.
///
/// A `shim` that keeps its batch, as one for an interpreted target does
/// whatever the policy asked for, is returned as is too: cmd.exe reads such a
/// shim to its end on every run, so the next link would lay it out once more.
///
/// [`CmdShimBatch::EndedBeforeTarget`]: super::CmdShimBatch::EndedBeforeTarget
#[must_use]
pub fn end_replaced_cmd_shim_batch(shim: &str, replaced: &str) -> String {
    if !ends_batch_before_target(shim) {
        return shim.to_string();
    }
    let Some(resume) = batch_resume(replaced) else {
        return shim.to_string();
    };
    let padding = resume.offset.checked_sub(JUMP_OVER.len()).and_then(rem_padding);
    match padding {
        Some(padding) => format!("{JUMP_OVER}{padding}{}{JUMP_TARGET}{shim}", resume.ending),
        None => shim.to_string(),
    }
}

/// Where cmd.exe reads on in a replaced shim once its target has exited.
struct BatchResume {
    /// The end of the last line before the code page restore when there is
    /// one, and of the last non-blank line otherwise.
    offset: usize,
    /// The lines the replaced batch runs from there: its code page restore,
    /// which carries its target's exit code past the restore, or
    /// [`END_REPLACED_BATCH`].
    ending: &'static str,
}

/// `None` when `shim` ends its batch before its target, or has no line at all.
fn batch_resume(shim: &str) -> Option<BatchResume> {
    if ends_batch_before_target(shim) {
        return None;
    }
    let (commands, ending) = match shim.find(CODEPAGE_RESTORE_TRAILER) {
        Some(trailer) => (&shim[..trailer], CODEPAGE_RESTORE_TRAILER),
        None => (shim, END_REPLACED_BATCH),
    };
    let mut end = 0;
    let mut offset = 0;
    for line in commands.split_inclusive('\n') {
        offset += line.len();
        if !line.trim().is_empty() {
            end = offset;
        }
    }
    (end > 0).then_some(BatchResume { offset: end, ending })
}

/// Whether `shim` is a [`CmdShimBatch::EndedBeforeTarget`] shim: one of its
/// lines, not merely a comment, is the [`BATCH_END`] line.
///
/// [`CmdShimBatch::EndedBeforeTarget`]: super::CmdShimBatch::EndedBeforeTarget
fn ends_batch_before_target(shim: &str) -> bool {
    shim.lines()
        .any(|line| line.starts_with(BATCH_END))
}

/// Exactly `len` bytes of `@REM` lines, the first carrying [`PADDING_NOTE`].
/// `None` when `len` is neither 0 nor room for a line.
fn rem_padding(len: usize) -> Option<String> {
    let mut padding = String::with_capacity(len);
    let mut remaining = len;
    while remaining > 0 {
        if remaining < REM_LINE_MIN {
            return None;
        }
        let mut line_len = remaining.min(REM_LINE_MAX);
        if (1..REM_LINE_MIN).contains(&(remaining - line_len)) {
            line_len -= REM_LINE_MIN;
        }
        let note = if padding.is_empty() { PADDING_NOTE } else { "" };
        padding.push_str(&rem_line(line_len, note));
        remaining -= line_len;
    }
    Some(padding)
}

/// One `@REM` line of exactly `len` bytes (at least [`REM_LINE_MIN`]), holding
/// as much of `note` as fits and spaces after it.
fn rem_line(len: usize, note: &str) -> String {
    let mut line = String::with_capacity(len);
    line.push_str("@REM");
    if let Some(room) = len.checked_sub("@REM \r\n".len()) {
        let fits = &note[..room.min(note.len())];
        line.push(' ');
        line.push_str(fits);
        line.extend(std::iter::repeat_n(' ', room - fits.len()));
    }
    line.push_str("\r\n");
    line
}
