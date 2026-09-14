//! Splitting a lockfile with Git conflict markers into its two valid sides.

const MERGE_CONFLICT_PARENT: &str = "|||||||";
const MERGE_CONFLICT_END: &str = ">>>>>>>";
const MERGE_CONFLICT_THEIRS: &str = "=======";
const MERGE_CONFLICT_OURS: &str = "<<<<<<<";

#[derive(Clone, Copy)]
enum Section {
    Common,
    Ours,
    Theirs,
    Parent,
}

pub(crate) fn split_git_conflict(file_content: &str) -> Option<(String, String)> {
    let mut section = Section::Common;
    let mut ours = String::with_capacity(file_content.len());
    let mut theirs = String::with_capacity(file_content.len());
    let mut saw_ours = false;
    let mut saw_theirs = false;
    let mut saw_end = false;

    for line in file_content.lines() {
        if line.starts_with(MERGE_CONFLICT_PARENT) {
            section = Section::Parent;
            continue;
        }
        if line.starts_with(MERGE_CONFLICT_OURS) {
            section = Section::Ours;
            saw_ours = true;
            continue;
        }
        if line == MERGE_CONFLICT_THEIRS {
            section = Section::Theirs;
            saw_theirs = true;
            continue;
        }
        if line.starts_with(MERGE_CONFLICT_END) {
            section = Section::Common;
            saw_end = true;
            continue;
        }
        if matches!(section, Section::Common | Section::Ours) {
            ours.push_str(line);
            ours.push('\n');
        }
        if matches!(section, Section::Common | Section::Theirs) {
            theirs.push_str(line);
            theirs.push('\n');
        }
    }

    (saw_ours && saw_theirs && saw_end).then_some((ours, theirs))
}

#[cfg(test)]
mod tests;
