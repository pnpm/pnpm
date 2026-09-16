//! Reading a lockfile Git left conflicted: splitting it into its two
//! valid sides and merging what each of them parses to.

use crate::LoadLockfileError;
use std::path::Path;

const MERGE_CONFLICT_PARENT: &str = "|||||||";
const MERGE_CONFLICT_END: &str = ">>>>>>>";
const MERGE_CONFLICT_THEIRS: &str = "=======";
pub(crate) const MERGE_CONFLICT_OURS: &str = "<<<<<<<";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Common,
    Ours,
    Parent,
    Theirs,
}

impl Section {
    /// The section `line` opens, or `None` when `line` is content rather
    /// than a marker. `=======` only counts on a line of its own, so a
    /// YAML scalar that merely starts with it stays content.
    fn opened_by(line: &str) -> Option<Self> {
        if line.starts_with(MERGE_CONFLICT_OURS) {
            return Some(Section::Ours);
        }
        if line.starts_with(MERGE_CONFLICT_PARENT) {
            return Some(Section::Parent);
        }
        if line == MERGE_CONFLICT_THEIRS {
            return Some(Section::Theirs);
        }
        if line.starts_with(MERGE_CONFLICT_END) {
            return Some(Section::Common);
        }
        None
    }

    /// Whether `next` may follow `self`, i.e. whether the marker that
    /// opens it is where Git would have written it.
    fn may_open(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Section::Common, Section::Ours)
                | (Section::Ours, Section::Parent | Section::Theirs)
                | (Section::Parent, Section::Theirs)
                | (Section::Theirs, Section::Common),
        )
    }
}

/// Whether `file_content` is a well-formed sequence of Git conflicts:
/// at least one, every marker where Git would have written it, and the
/// last one closed.
///
/// Checked before either side is built, so a file that merely failed to
/// parse — the common case, since recovery is only ever tried on a parse
/// failure — costs a scan of its lines rather than a second and third
/// copy of a lockfile that can run to hundreds of megabytes.
fn is_git_conflicted(file_content: &str) -> bool {
    let mut section = Section::Common;
    let mut conflicts = 0_usize;
    for next in file_content.lines().filter_map(Section::opened_by) {
        if !section.may_open(next) {
            return false;
        }
        conflicts += usize::from(next == Section::Ours);
        section = next;
    }
    section == Section::Common && conflicts > 0
}

/// Rebuild the two sides line by line. The diff3 parent section belongs
/// to neither, so it is dropped as it is read.
fn build_conflict_sides(file_content: &str) -> (String, String) {
    let mut section = Section::Common;
    let mut ours = String::with_capacity(file_content.len());
    let mut theirs = String::with_capacity(file_content.len());
    for line in file_content.lines() {
        if let Some(next) = Section::opened_by(line) {
            section = next;
            continue;
        }
        if matches!(section, Section::Common | Section::Ours) {
            push_line(&mut ours, line);
        }
        if matches!(section, Section::Common | Section::Theirs) {
            push_line(&mut theirs, line);
        }
    }
    (ours, theirs)
}

fn push_line(side: &mut String, line: &str) {
    side.push_str(line);
    side.push('\n');
}

/// Split a file Git left conflicted into the "ours" and "theirs" documents
/// it was merged from.
///
/// Returns `None` unless the input passes [`is_git_conflicted`]. A file
/// that only looks conflicted — a stray marker, an unterminated conflict
/// — yields nothing, so the caller keeps reporting it as the broken
/// lockfile it is instead of merging two documents whose halves were cut
/// at the wrong lines.
pub(crate) fn split_git_conflict(file_content: &str) -> Option<(String, String)> {
    is_git_conflicted(file_content).then(|| build_conflict_sides(file_content))
}

/// What a wanted-lockfile file parsed to, and how many files the parse
/// had to merge Git conflict markers out of to get there.
pub(crate) struct ParsedWantedFile<Parsed> {
    pub(crate) value: Option<Parsed>,
    pub(crate) merged_conflict_files: usize,
}

/// Parse a lockfile file, recovering from the Git conflict markers a
/// merge left in it by parsing both sides and merging the results.
///
/// The recovery only runs once the file has failed to parse as it stands,
/// and a side that does not parse on its own leaves the original parse
/// error in place: a file only looks conflicted until both of the
/// documents it was merged from are in hand, and the side's own error
/// would point at line numbers no file on disk has.
///
/// The split is of the whole `content`, before any YAML document is
/// selected out of it, so `parse` sees each side as a complete file.
/// A combined lockfile can be conflicted in either of its documents, or
/// across the `---` that separates them, and only a side cut from the
/// whole file is the file that branch actually had.
pub(crate) fn parse_wanted_file<Parsed>(
    content: &str,
    file_path: &Path,
    parse: impl Fn(&str, &Path) -> Result<Option<Parsed>, LoadLockfileError>,
    merge: impl FnOnce(&Parsed, &Parsed) -> Parsed,
) -> Result<ParsedWantedFile<Parsed>, LoadLockfileError> {
    let parse_error = match parse(content, file_path) {
        Ok(value) => return Ok(ParsedWantedFile { value, merged_conflict_files: 0 }),
        Err(error) => error,
    };
    let Some((ours, theirs)) = split_git_conflict(content) else { return Err(parse_error) };
    let Ok(Some(ours)) = parse(&ours, file_path) else { return Err(parse_error) };
    let Ok(Some(theirs)) = parse(&theirs, file_path) else { return Err(parse_error) };
    Ok(ParsedWantedFile { value: Some(merge(&ours, &theirs)), merged_conflict_files: 1 })
}

#[cfg(test)]
mod tests;
