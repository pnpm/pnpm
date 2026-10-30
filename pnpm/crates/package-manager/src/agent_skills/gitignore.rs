//! The `# pnpm` block of each agent skill directory's `.gitignore`.

use super::{AgentSkillsError, is_within_workspace};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

/// The ignore rule that older pnpm versions added for links named
/// `pnpm-<package>-<skill>`.
const LEGACY_GITIGNORE_RULE: &str = "pnpm-*";

/// The name of the `.gitignore` block that lists pnpm's entries.
const GITIGNORE_SECTION: &str = "pnpm";

/// Keep pnpm's entries out of commits, because they point into
/// `node_modules` and would dangle in a fresh clone. The `.gitignore` of
/// each directory in `dirs`, or that `linked` or `desired` has entries in,
/// gets a `# pnpm` block listing exactly the entries `desired` has there.
pub(super) fn update_gitignores(
    workspace_root: &Path,
    dirs: &[PathBuf],
    linked: &[String],
    desired: &BTreeMap<String, PathBuf>,
) -> Result<(), AgentSkillsError> {
    let mut by_dir: BTreeMap<PathBuf, BTreeSet<String>> = dirs
        .iter()
        .map(|dir| (dir.clone(), BTreeSet::new()))
        .collect();
    for entry in linked.iter().filter(|entry| is_within_workspace(entry)) {
        if let Some(dir) = Path::new(entry).parent() {
            by_dir
                .entry(workspace_root.join(dir))
                .or_default();
        }
    }
    for entry in desired.keys() {
        let path = Path::new(entry);
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { continue };
        by_dir
            .entry(workspace_root.join(dir))
            .or_default()
            .insert(gitignore_rule(&name.to_string_lossy()));
    }
    for (dir, rules) in by_dir {
        update_gitignore(&dir.join(".gitignore"), &rules)?;
    }
    Ok(())
}

/// Rewrite the `# pnpm` block of the `.gitignore` at `path` to list
/// `rules`. The file is created only when there is a rule to list, and
/// removed when no line remains.
fn update_gitignore(path: &Path, rules: &BTreeSet<String>) -> Result<(), AgentSkillsError> {
    let io_error = |source| AgentSkillsError::Io { path: path.to_path_buf(), source };
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(io_error(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cannot write through a symlinked .gitignore",
        )));
    }
    let existing = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound && rules.is_empty() => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(io_error(source)),
    };
    let updated = updated_gitignore(&existing, rules);
    if updated == existing {
        return Ok(());
    }
    if updated.is_empty() {
        return fs::remove_file(path).map_err(io_error);
    }
    fs::write(path, updated).map_err(io_error)
}

/// `existing` with its first `# pnpm` block listing `rules`, or with one
/// appended, and without any other `# pnpm` block or the legacy rule. An
/// empty `rules` removes the block.
fn updated_gitignore(existing: &str, rules: &BTreeSet<String>) -> String {
    let block = (!rules.is_empty()).then(|| {
        let body = rules
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        pnpm_text_section::wrap_section(GITIGNORE_SECTION, &body)
    });
    let sections = pnpm_text_section::find_sections(existing, GITIGNORE_SECTION);
    let mut updated = String::new();
    let mut cursor = 0;
    for (index, section) in sections.iter().enumerate() {
        updated.push_str(&existing[cursor..section.range.start]);
        if let (0, Some(block)) = (index, &block) {
            updated.push_str(block);
            cursor = section.range.end;
        } else {
            cursor = end_of_line(existing, section.range.end);
        }
    }
    updated.push_str(&existing[cursor..]);
    let mut lines: Vec<&str> = updated
        .lines()
        .filter(|line| line.trim_end() != LEGACY_GITIGNORE_RULE)
        .collect();
    if sections.is_empty()
        && let Some(block) = &block
    {
        lines.push(block);
    }
    lines
        .iter()
        .flat_map(|line| [*line, "\n"])
        .collect()
}

/// The offset after the line break that follows `offset`, or the end of
/// `text`.
fn end_of_line(text: &str, offset: usize) -> usize {
    text[offset..]
        .find('\n')
        .map_or(text.len(), |position| offset + position + 1)
}

/// The `.gitignore` rule that matches exactly the entry `name` of its
/// directory.
fn gitignore_rule(name: &str) -> String {
    let mut rule = String::from("/");
    for character in name.chars() {
        if matches!(character, '\\' | '*' | '?' | '[' | ']' | ' ' | '!' | '#') {
            rule.push('\\');
        }
        rule.push(character);
    }
    rule
}
