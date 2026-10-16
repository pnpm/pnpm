//! Port of [`ignore-walk`](https://github.com/npm/ignore-walk) as
//! npm-packlist configures it. Every directory's rules see each entry
//! together with the rules of its ancestors, so a negation such as
//! `!lib/**` re-includes files under a directory that `*` excluded.

mod default_rules;
mod rule;
mod segment;
#[cfg(test)]
mod tests;

use crate::ALWAYS_EXCLUDED_DIR_SEGMENTS;
use default_rules::DirDefaults;
use rule::IgnoreRule;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Component, Path},
};

/// Which ignore files the walk reads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum IgnoreFiles<'a> {
    /// Each subdirectory's, as for [`Self::Read`], but not the package
    /// root's or any inherited ones, and not the default rules at the root:
    /// a `files` allowlist takes their place.
    /// The walk keeps `named_files`, package-relative paths with `/`
    /// separators, even where a subdirectory's rules exclude them.
    SubdirectoriesOnly { named_files: &'a BTreeSet<String> },
    /// Each directory's `.npmignore`, or its `.gitignore` when it has no
    /// `.npmignore`. For a package inside `workspace_dir` that has no
    /// `.npmignore`, the ignore files of the directories from `workspace_dir`
    /// down to the package's parent apply too, matched against paths
    /// relative to the package.
    Read {
        workspace_dir: Option<&'a Path>,
        /// Package-relative paths that the root re-includes after its ignore
        /// files, see [`IgnoreRule::reinclude_path`].
        required_paths: &'a [String],
    },
}

/// Calls `on_file` with every non-directory entry under `pkg_dir` that the
/// ignore rules keep. It does not descend into a top-level `node_modules` or
/// into a version-control directory, and does not follow symlinks.
pub(crate) fn walk_files(
    pkg_dir: &Path,
    ignore_files: IgnoreFiles<'_>,
    on_file: &mut dyn FnMut(&Path, fs::FileType),
) -> io::Result<()> {
    let (root_rules, root_defaults, named_files) = match ignore_files {
        IgnoreFiles::SubdirectoriesOnly { named_files } => {
            (Vec::new(), DirDefaults::NONE, Some(named_files))
        }
        IgnoreFiles::Read { workspace_dir, required_paths } => {
            let mut rules = inherited_rules(pkg_dir, workspace_dir)?;
            rules.extend(read_dir_rules(pkg_dir)?);
            rules.extend(required_paths.iter().map(|path| IgnoreRule::reinclude_path(path)));
            (rules, DirDefaults::root(), None)
        }
    };
    let root = DirRules {
        parent: None,
        name: String::new(),
        rel_path: String::new(),
        named_only: false,
        any_rules: !root_rules.is_empty(),
        defaults: root_defaults,
        rules: root_rules,
        exact: false,
    };
    let mut walker = Walker { named_files: named_files.filter(|named| !named.is_empty()), on_file };
    walker.walk_dir(pkg_dir, &root, 0)
}

struct Walker<'a> {
    named_files: Option<&'a BTreeSet<String>>,
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
        let included = rules.included(&name);
        let kept = self.kept(rules, &name, included);
        if !kept.as_file && !kept.as_dir {
            return Ok(());
        }
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            if kept.as_file {
                (self.on_file)(&entry.path(), file_type);
            }
            return Ok(());
        }
        if kept.as_dir && !is_pruned(&name, depth) {
            self.walk_child_dir(&entry.path(), rules, name, included, depth)?;
        }
        Ok(())
    }

    /// What the walk keeps of the entry `name` of the directory `dir`: what
    /// its rules include, plus a named file and the directories leading to one.
    fn kept(&self, dir: &DirRules<'_>, name: &str, included: Included) -> Included {
        if included.as_file && included.as_dir {
            return included;
        }
        let named = self.named(dir, name);
        Included {
            as_file: included.as_file || named == Named::File,
            as_dir: included.as_dir || named == Named::Ancestor,
        }
    }

    /// How `named_files` refers to the entry `name` of the directory `dir`.
    fn named(&self, dir: &DirRules<'_>, name: &str) -> Named {
        let Some(named_files) = self.named_files else { return Named::No };
        let rel_path = dir.child_rel_path(name);
        if named_files.contains(&rel_path) {
            return Named::File;
        }
        let dir_prefix = format!("{rel_path}/");
        let holds_named = named_files
            .range(dir_prefix.clone()..)
            .next()
            .is_some_and(|path| path.starts_with(&dir_prefix));
        if holds_named { Named::Ancestor } else { Named::No }
    }

    fn walk_child_dir(
        &mut self,
        path: &Path,
        parent: &DirRules<'_>,
        name: String,
        included: Included,
        depth: usize,
    ) -> io::Result<()> {
        let rules = read_dir_rules(path)?;
        let defaults = parent.defaults.child(&name);
        let exact = included.as_file
            || parent.includes(&format!("{name}/"), false, None, defaults.has_candidate_segment);
        let child = DirRules {
            parent: Some(parent),
            rel_path: parent.child_rel_path(&name),
            name,
            any_rules: parent.any_rules || !rules.is_empty(),
            defaults,
            rules,
            exact,
            named_only: !included.as_dir,
        };
        self.walk_dir(path, &child, depth + 1)
    }
}

/// Whether a directory's rules include an entry, as a file and as a
/// directory that may hold included paths.
#[derive(Debug, Clone, Copy)]
struct Included {
    as_file: bool,
    as_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Named {
    No,
    File,
    /// A directory that holds a named file.
    Ancestor,
}

fn is_pruned(name: &str, depth: usize) -> bool {
    (depth == 0 && name == "node_modules") || ALWAYS_EXCLUDED_DIR_SEGMENTS.contains(&name)
}

/// The rules of one directory on the walk, linked to its ancestors'.
struct DirRules<'a> {
    parent: Option<&'a DirRules<'a>>,
    /// The directory's name in its parent.
    name: String,
    /// The directory's path relative to the package, `/`-separated.
    rel_path: String,
    /// Evaluated before `rules`.
    defaults: DirDefaults,
    rules: Vec<IgnoreRule>,
    /// Whether the parent includes the directory itself, not only paths
    /// below it. Only then may the directory's own rules re-include an entry
    /// its ancestors exclude.
    exact: bool,
    /// Whether this directory or an ancestor has any rule.
    any_rules: bool,
    /// Whether the walk entered the directory only for the named files
    /// below it, which are then the only entries it keeps.
    named_only: bool,
}

impl DirRules<'_> {
    fn included(&self, entry: &str) -> Included {
        if self.named_only {
            return Included { as_file: false, as_dir: false };
        }
        let may_match_defaults = self.defaults.entry_may_match(entry);
        Included {
            as_file: self.includes(entry, false, None, may_match_defaults),
            as_dir: self.includes(entry, true, None, may_match_defaults),
        }
    }

    fn child_rel_path(&self, name: &str) -> String {
        if self.rel_path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{name}", self.rel_path)
        }
    }

    /// ignore-walk's `filterEntry`. `partial` asks whether `entry`, as a
    /// directory, may hold included paths. `entry_basename` is the name of
    /// the entry being walked when `entry` is a path from a descendant.
    /// `may_match_defaults` is whether the entry's package-relative path has a
    /// segment the default rules may match; without one, only user rules apply.
    fn includes(
        &self,
        entry: &str,
        partial: bool,
        entry_basename: Option<&str>,
        may_match_defaults: bool,
    ) -> bool {
        if !self.any_rules && !may_match_defaults {
            return true;
        }
        let mut included = true;
        if let Some(parent) = self.parent {
            let parent_entry = format!("{}/{entry}", self.name);
            included = parent.includes(
                &parent_entry,
                partial,
                Some(entry_basename.unwrap_or(entry)),
                may_match_defaults,
            );
            if !included && !self.exact {
                return false;
            }
        }
        for rule in self.defaults
            .rules(may_match_defaults)
            .iter()
            .chain(&self.rules)
        {
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
