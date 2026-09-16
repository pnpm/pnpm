//! Reading a lockfile Git left conflicted: splitting it into its two
//! valid sides and merging what each of them parses to.

use crate::{LoadLockfileError, extract_main_document};
use std::path::Path;

const MERGE_CONFLICT_PARENT: &str = "|||||||";
const MERGE_CONFLICT_END: &str = ">>>>>>>";
const MERGE_CONFLICT_THEIRS: &str = "=======";
const MERGE_CONFLICT_OURS: &str = "<<<<<<<";

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

/// The two sides being rebuilt line by line. The diff3 parent section
/// belongs to neither, so it is dropped as it is read.
struct ConflictSides {
    ours: String,
    theirs: String,
}

impl ConflictSides {
    fn with_capacity(capacity: usize) -> Self {
        ConflictSides {
            ours: String::with_capacity(capacity),
            theirs: String::with_capacity(capacity),
        }
    }

    fn push(&mut self, section: Section, line: &str) {
        if matches!(section, Section::Common | Section::Ours) {
            push_line(&mut self.ours, line);
        }
        if matches!(section, Section::Common | Section::Theirs) {
            push_line(&mut self.theirs, line);
        }
    }
}

fn push_line(side: &mut String, line: &str) {
    side.push_str(line);
    side.push('\n');
}

/// Split a file Git left conflicted into the "ours" and "theirs" documents
/// it was merged from.
///
/// Returns `None` unless the whole input is a well-formed sequence of
/// conflicts: at least one, every marker where Git would have written it,
/// and the last one closed. A file that only looks conflicted — a stray
/// marker, an unterminated conflict — yields nothing, so the caller keeps
/// reporting it as the broken lockfile it is instead of merging two
/// documents whose halves were cut at the wrong lines.
pub(crate) fn split_git_conflict(file_content: &str) -> Option<(String, String)> {
    let mut section = Section::Common;
    let mut sides = ConflictSides::with_capacity(file_content.len());
    let mut conflicts = 0_usize;

    for line in file_content.lines() {
        let Some(next) = Section::opened_by(line) else {
            sides.push(section, line);
            continue;
        };
        if !section.may_open(next) {
            return None;
        }
        conflicts += usize::from(next == Section::Ours);
        section = next;
    }

    (section == Section::Common && conflicts > 0).then_some((sides.ours, sides.theirs))
}

/// What a wanted-lockfile file parsed to, and how many files the parse
/// had to merge Git conflict markers out of to get there.
pub(crate) struct ParsedWantedFile<Parsed> {
    pub(crate) value: Option<Parsed>,
    pub(crate) merged_conflict_files: usize,
}

/// Parse a wanted-lockfile file, recovering from the Git conflict markers
/// a merge left in it by parsing both sides and merging the results.
///
/// The recovery only runs once the file has failed to parse as it stands,
/// and a side that does not parse on its own leaves the original parse
/// error in place: a file only looks conflicted until both of the
/// documents it was merged from are in hand.
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
    let main = extract_main_document(content);
    let Some((ours, theirs)) = split_git_conflict(&main) else { return Err(parse_error) };
    let Some(ours) = parse(&ours, file_path)? else { return Err(parse_error) };
    let Some(theirs) = parse(&theirs, file_path)? else { return Err(parse_error) };
    Ok(ParsedWantedFile { value: Some(merge(&ours, &theirs)), merged_conflict_files: 1 })
}

#[cfg(test)]
mod tests;
