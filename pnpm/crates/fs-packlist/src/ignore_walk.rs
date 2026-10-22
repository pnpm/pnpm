//! Port of [`ignore-walk`](https://github.com/npm/ignore-walk) as
//! npm-packlist configures it. Every directory's rules see each entry
//! together with the rules of its ancestors, so a negation such as
//! `!lib/**` re-includes files under a directory that `*` excluded.

pub(crate) mod files_field;

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

#[derive(Clone, Copy)]
pub(crate) struct PackageRules<'a> {
    pub workspace_dir: Option<&'a Path>,
    pub manifest: &'a serde_json::Value,
    /// Alternate manifests are required only for the package being packed.
    pub include_alternate_manifests: bool,
}

/// Calls `on_file` with every non-directory entry under `pkg_dir` that the
/// ignore rules keep. It does not descend into a top-level `node_modules` or
/// into a version-control directory, and does not follow symlinks.
pub(crate) fn walk_files(
    pkg_dir: &Path,
    options: PackageRules<'_>,
    on_file: &mut dyn FnMut(&Path, fs::FileType),
) -> io::Result<()> {
    let matcher = root_rules(pkg_dir, options)?;
    let root_rules = matcher.rules;
    let root = DirRules {
        parent: None,
        name: String::new(),
        named_only: false,
        rel_path: String::new(),
        has_exclusions: root_rules.iter().any(|rule| !rule.negate),
        defaults: DirDefaults::root(),
        rules: root_rules,
        exact: false,
    };
    let mut walker = Walker {
        named_files: (!matcher.named_files.is_empty()).then_some(&matcher.named_files),
        on_file,
    };
    walker.walk_dir(pkg_dir, &root, 0)
}

fn root_rules(pkg_dir: &Path, options: PackageRules<'_>) -> io::Result<files_field::FilesMatcher> {
    let files = options.manifest.get("files").and_then(serde_json::Value::as_array);
    let mut matcher =
        files_field::compile_files_rules(pkg_dir, files.map_or(&[][..], Vec::as_slice));
    if files.is_none() {
        matcher.rules = inherited_rules(pkg_dir, options.workspace_dir)?;
        matcher.rules.extend(read_dir_rules(pkg_dir)?);
    }
    matcher.rules.extend(root_required_rules());
    matcher.rules.extend(
        crate::required_field_paths(options.manifest)
            .iter()
            .map(|path| IgnoreRule::reinclude_path(path)),
    );
    if let Some(config_path) = crate::pnpm_config_path(options.manifest) {
        matcher.named_files.insert(config_path);
    }
    if options.include_alternate_manifests {
        matcher.rules.extend(["!/package.yaml", "!/package.json5"].map(
            IgnoreRule::parse_ascii_case,
        ));
    }
    Ok(matcher)
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
        if name == ".npmrc" || is_pruned(&name, depth) {
            return Ok(());
        }
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
        if kept.as_dir {
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
        let rel_path = parent.child_rel_path(&name);
        let defaults = parent.defaults.child(&name);
        let exact = included.as_file
            || parent.includes(&format!("{name}/"), false, None, defaults.has_candidate_segment);
        let child = DirRules {
            parent: Some(parent),
            rel_path,
            named_only: !included.as_dir,
            name,
            has_exclusions: parent.has_exclusions || rules.iter().any(|rule| !rule.negate),
            defaults,
            rules,
            exact,
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
    (depth == 0
        && matches!(
            name,
            "node_modules" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "bun.lockb",
        ))
        || ALWAYS_EXCLUDED_DIR_SEGMENTS.contains(&name)
}

/// The rules of one directory on the walk, linked to its ancestors'.
struct DirRules<'a> {
    /// Only explicitly named files may be kept below an excluded directory.
    named_only: bool,
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
    /// Whether this directory or an ancestor has an exclusion rule.
    has_exclusions: bool,
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
        if !self.has_exclusions && !may_match_defaults {
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
        let rules = self.defaults
            .rules(may_match_defaults)
            .iter()
            .chain(&self.rules);
        apply_rules(rules, included, entry, partial, entry_basename)
    }
}

/// Folds `rules` over `included`: each matching rule that flips it wins, so
/// the last match decides.
fn apply_rules<'a>(
    rules: impl Iterator<Item = &'a IgnoreRule>,
    included: bool,
    entry: &str,
    partial: bool,
    entry_basename: Option<&str>,
) -> bool {
    rules.fold(included, |included, rule| {
        if rule.negate != included && rule_matches(rule, entry, partial, entry_basename) {
            rule.negate
        } else {
            included
        }
    })
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

fn root_required_rules() -> impl Iterator<Item = IgnoreRule> {
    [
        "!/package.json",
        "!/readme{,.*[^~$]}",
        "!/copying{,.*[^~$]}",
        "!/license{,.*[^~$]}",
        "!/licence{,.*[^~$]}",
    ]
    .into_iter()
    .map(IgnoreRule::parse_ascii_case)
}
