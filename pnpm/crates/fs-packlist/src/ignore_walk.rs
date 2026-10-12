//! Port of [`ignore-walk`](https://github.com/npm/ignore-walk) as
//! npm-packlist configures it. Every directory's rules see each entry
//! together with the rules of its ancestors, so a negation such as
//! `!lib/**` re-includes files under a directory that `*` excluded.

mod rule;
mod segment;
#[cfg(test)]
mod tests;

use crate::ALWAYS_EXCLUDED_DIR_SEGMENTS;
use rule::IgnoreRule;
use std::{
    fs, io,
    path::{Component, Path},
};

/// Which ignore files the walk reads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum IgnoreFiles<'a> {
    /// None: a `files` allowlist decides what ships.
    Skip,
    /// Each directory's `.npmignore`, or its `.gitignore` when it has no
    /// `.npmignore`. For a package inside `workspace_dir` that has no
    /// `.npmignore`, the ignore files of the directories from `workspace_dir`
    /// down to the package's parent apply too, matched against paths
    /// relative to the package.
    Read { workspace_dir: Option<&'a Path> },
}

/// Calls `on_file` with every non-directory entry under `pkg_dir` that the
/// ignore rules keep. It does not descend into a top-level `node_modules` or
/// into a version-control directory, and does not follow symlinks.
pub(crate) fn walk_files(
    pkg_dir: &Path,
    ignore_files: IgnoreFiles<'_>,
    on_file: &mut dyn FnMut(&Path, fs::FileType),
) -> io::Result<()> {
    let root_rules = match ignore_files {
        IgnoreFiles::Skip => Vec::new(),
        IgnoreFiles::Read { workspace_dir } => {
            let mut rules = inherited_rules(pkg_dir, workspace_dir)?;
            rules.extend(read_dir_rules(pkg_dir)?);
            rules
        }
    };
    let root = DirRules {
        parent: None,
        name: String::new(),
        any_rules: !root_rules.is_empty(),
        rules: root_rules,
        exact: false,
    };
    let mut walker =
        Walker { read_ignore_files: matches!(ignore_files, IgnoreFiles::Read { .. }), on_file };
    walker.walk_dir(pkg_dir, &root, 0)
}

struct Walker<'a> {
    read_ignore_files: bool,
    on_file: &'a mut dyn FnMut(&Path, fs::FileType),
}

impl Walker<'_> {
    fn walk_dir(&mut self, dir: &Path, rules: &DirRules<'_>, depth: usize) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            self.visit_entry(&entry?, rules, depth)?;
        }
        Ok(())
    }

    fn visit_entry(
        &mut self,
        entry: &fs::DirEntry,
        rules: &DirRules<'_>,
        depth: usize,
    ) -> io::Result<()> {
        let name = entry
            .file_name()
            .to_string_lossy()
            .into_owned();
        let as_file = rules.includes(&name, false, None);
        let as_dir = rules.includes(&name, true, None);
        if !as_file && !as_dir {
            return Ok(());
        }
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            if as_file {
                (self.on_file)(&entry.path(), file_type);
            }
            return Ok(());
        }
        if as_dir && !is_pruned(&name, depth) {
            self.walk_child_dir(&entry.path(), rules, name, as_file, depth)?;
        }
        Ok(())
    }

    fn walk_child_dir(
        &mut self,
        path: &Path,
        parent: &DirRules<'_>,
        name: String,
        included_as_file: bool,
        depth: usize,
    ) -> io::Result<()> {
        let rules = if self.read_ignore_files { read_dir_rules(path)? } else { Vec::new() };
        let exact = included_as_file || parent.includes(&format!("{name}/"), false, None);
        let child = DirRules {
            parent: Some(parent),
            name,
            any_rules: parent.any_rules || !rules.is_empty(),
            rules,
            exact,
        };
        self.walk_dir(path, &child, depth + 1)
    }
}

fn is_pruned(name: &str, depth: usize) -> bool {
    (depth == 0 && name == "node_modules") || ALWAYS_EXCLUDED_DIR_SEGMENTS.contains(&name)
}

/// The rules of one directory on the walk, linked to its ancestors'.
struct DirRules<'a> {
    parent: Option<&'a DirRules<'a>>,
    /// The directory's name in its parent.
    name: String,
    rules: Vec<IgnoreRule>,
    /// Whether the parent includes the directory itself, not only paths
    /// below it. Only then may the directory's own rules re-include an entry
    /// its ancestors exclude.
    exact: bool,
    /// Whether this directory or an ancestor has any rule.
    any_rules: bool,
}

impl DirRules<'_> {
    /// ignore-walk's `filterEntry`. `partial` asks whether `entry`, as a
    /// directory, may hold included paths. `entry_basename` is the name of
    /// the entry being walked when `entry` is a path from a descendant.
    fn includes(&self, entry: &str, partial: bool, entry_basename: Option<&str>) -> bool {
        if !self.any_rules {
            return true;
        }
        let mut included = true;
        if let Some(parent) = self.parent {
            let parent_entry = format!("{}/{entry}", self.name);
            included =
                parent.includes(&parent_entry, partial, Some(entry_basename.unwrap_or(entry)));
            if !included && !self.exact {
                return false;
            }
        }
        for rule in &self.rules {
            if rule.negate != included && rule_matches(rule, entry, partial, entry_basename) {
                included = rule.negate;
            }
        }
        included
    }
}

fn rule_matches(
    rule: &IgnoreRule,
    entry: &str,
    partial: bool,
    entry_basename: Option<&str>,
) -> bool {
    if rule.matches(&format!("/{entry}"), false) || rule.matches(entry, false) {
        return true;
    }
    if !partial {
        return false;
    }
    if matches_as_dir(rule, entry) {
        return true;
    }
    entry_basename.is_some_and(|basename| rule.is_relative() && matches_as_dir(rule, basename))
}

fn matches_as_dir(rule: &IgnoreRule, entry: &str) -> bool {
    rule.matches(&format!("/{entry}/"), false)
        || rule.matches(&format!("{entry}/"), false)
        || (rule.negate && (rule.matches(&format!("/{entry}"), true) || rule.matches(entry, true)))
}

/// The rules of `dir`'s `.npmignore`, or of its `.gitignore` when it has no
/// `.npmignore`.
fn read_dir_rules(dir: &Path) -> io::Result<Vec<IgnoreRule>> {
    let npmignore = dir.join(".npmignore");
    let ignore_file = if npmignore.is_file() { npmignore } else { dir.join(".gitignore") };
    if !ignore_file.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read(&ignore_file)?;
    Ok(String::from_utf8_lossy(&content)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(IgnoreRule::parse)
        .collect())
}

fn inherited_rules(pkg_dir: &Path, workspace_dir: Option<&Path>) -> io::Result<Vec<IgnoreRule>> {
    let mut rules = Vec::new();
    let Some(workspace_dir) = workspace_dir else { return Ok(rules) };
    if pkg_dir.join(".npmignore").is_file() {
        return Ok(rules);
    }
    let Some(parent_rel) = pkg_dir
        .parent()
        .and_then(|parent| parent.strip_prefix(workspace_dir).ok())
    else {
        return Ok(rules);
    };
    let mut current = workspace_dir.to_path_buf();
    rules.extend(read_dir_rules(&current)?);
    for component in parent_rel.components() {
        // A component other than `Normal` means a non-canonical path; stop
        // rather than read ignore files outside the workspace.
        let Component::Normal(segment) = component else { break };
        current.push(segment);
        rules.extend(read_dir_rules(&current)?);
    }
    Ok(rules)
}
