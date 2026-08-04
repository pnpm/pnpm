//! Hunk application with pnpm's tolerances.
//!
//! pnpm applies patches through `@pnpm/patch-package`, whose matcher is
//! deliberately loose in two ways [`diffy::apply`] is not, and real
//! patch files in the wild depend on both:
//!
//! 1. Lines are compared with trailing whitespace stripped, so an LF
//!    patch applies to a CRLF file (and a hunk whose last line is
//!    context applies to a file with no final newline).
//! 2. A hunk that doesn't match at its recorded position is retried up
//!    to twenty lines either side of it.
//!
//! The file is modeled as `split('\n')` throughout — the same
//! representation `patch-package` uses — so a line's own ending is
//! whatever the surrounding text carried: untouched CRLF lines keep
//! their `\r`, replacement lines take the patch's LF, and a file
//! without a final newline keeps that shape through the round trip.

use diffy::{Line, Patch};

/// How far either side of its recorded position a hunk is retried
/// before the patch is rejected.
const MAX_FUZZING_OFFSET: isize = 20;

/// Apply every hunk of `patch` to `original`.
///
/// Hunks are located against the unpatched file and only then spliced
/// in, so one hunk's edits can't shift another out from under its own
/// match — the recorded line numbers all address the same baseline.
///
/// The error carries `diffy`'s wording so the diagnostic a failed patch
/// produces stays the same whichever matcher rejected it.
pub(super) fn apply(original: &str, patch: &Patch<'_, str>) -> Result<String, String> {
    let mut lines: Vec<&str> = original.split('\n').collect();

    let mut modifications = Vec::new();
    for (index, hunk) in patch.hunks().iter().enumerate() {
        let parts = split_into_parts(hunk.lines());
        let start = to_isize(hunk.old_range().start());
        let matched = fuzzing_offsets()
            .find_map(|offset| {
                evaluate_hunk(&parts, &lines, start - 1 + offset, hunk.old_range().len())
            })
            .ok_or_else(|| format!("error applying hunk #{}", index + 1))?;
        modifications.extend(matched);
    }

    let mut offset = 0_isize;
    for modification in modifications {
        match modification {
            Modification::Splice { index, delete, insert } => {
                // Clamped rather than trusted: the positions were found
                // against the unpatched file, so a patch whose hunks
                // overlap can shift one past the end. JavaScript's
                // `splice` clamps too, and a patch file is untrusted
                // input — an out-of-range index must not panic.
                let at = to_usize(to_isize(index) + offset).min(lines.len());
                offset += to_isize(insert.len()) - to_isize(delete);
                lines.splice(at..(at + delete).min(lines.len()), insert);
            }
            Modification::Pop => {
                lines.pop();
            }
            Modification::Push => lines.push(""),
        }
    }

    Ok(lines.join("\n"))
}

/// The positions a hunk is tried at, relative to its recorded one:
/// `0, -1, 1, -2, 2, …` out to [`MAX_FUZZING_OFFSET`].
fn fuzzing_offsets() -> impl Iterator<Item = isize> {
    std::iter::once(0).chain((1..=MAX_FUZZING_OFFSET).flat_map(|offset| [-offset, offset]))
}

/// A run of consecutive same-kind lines within a hunk, holding each
/// line without its ending. `ends_file` marks the run whose last line
/// carried the `\ No newline at end of file` annotation — the pre-image
/// (for a deletion) or post-image (for an insertion) ends there without
/// a final newline.
struct Part<'a> {
    kind: Kind,
    lines: Vec<&'a str>,
    ends_file: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Context,
    Delete,
    Insert,
}

/// An edit to the `split('\n')` line list, recorded while matching and
/// replayed afterwards. `Push` / `Pop` add and remove the trailing
/// empty element that stands for the file's final newline.
enum Modification<'a> {
    Splice { index: usize, delete: usize, insert: Vec<&'a str> },
    Pop,
    Push,
}

fn split_into_parts<'a>(lines: &[Line<'a, str>]) -> Vec<Part<'a>> {
    let mut parts: Vec<Part<'a>> = Vec::new();
    for line in lines {
        let (kind, text) = match *line {
            Line::Context(text) => (Kind::Context, text),
            Line::Delete(text) => (Kind::Delete, text),
            Line::Insert(text) => (Kind::Insert, text),
        };
        let ends_file = !text.ends_with('\n');
        let text = text.strip_suffix('\n').unwrap_or(text);
        match parts.last_mut() {
            Some(part) if part.kind == kind => {
                part.lines.push(text);
                part.ends_file = ends_file;
            }
            _ => parts.push(Part { kind, lines: vec![text], ends_file }),
        }
    }
    parts
}

/// Match `parts` against `lines` starting at `start`, returning the
/// edits that applying the hunk there implies, or `None` when the
/// context doesn't line up.
fn evaluate_hunk<'a>(
    parts: &[Part<'a>],
    lines: &[&str],
    start: isize,
    pre_image_len: usize,
) -> Option<Vec<Modification<'a>>> {
    if start < 0 {
        return None;
    }
    let mut index = to_usize(start);
    if lines.len().checked_sub(index)? < pre_image_len {
        return None;
    }

    let mut modifications = Vec::new();
    for part in parts {
        match part.kind {
            Kind::Context | Kind::Delete => {
                for line in &part.lines {
                    if !lines_are_equal(lines.get(index)?, line) {
                        return None;
                    }
                    index += 1;
                }
                if part.kind == Kind::Delete {
                    modifications.push(Modification::Splice {
                        index: index - part.lines.len(),
                        delete: part.lines.len(),
                        insert: Vec::new(),
                    });
                    // The deleted run ended the pre-image without a
                    // newline; unless an insertion says otherwise, the
                    // post-image gets one.
                    if part.ends_file {
                        modifications.push(Modification::Push);
                    }
                }
            }
            Kind::Insert => {
                modifications.push(Modification::Splice {
                    index,
                    delete: 0,
                    insert: part.lines.clone(),
                });
                if part.ends_file {
                    modifications.push(Modification::Pop);
                }
            }
        }
    }
    Some(modifications)
}

/// Trailing whitespace is ignored, which is what lets an LF patch match
/// a CRLF file: the `\r` the file carries is whitespace the patch's
/// context lines don't have.
fn lines_are_equal(file_line: &str, patch_line: &str) -> bool {
    file_line.trim_end() == patch_line.trim_end()
}

fn to_isize(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

fn to_usize(value: isize) -> usize {
    usize::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests;
