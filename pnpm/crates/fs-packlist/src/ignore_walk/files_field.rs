//! Compile the manifest allowlist into the walk's ignore rules.

use super::{IgnoreRule, apply_rules, rule_matches};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

/// Compiled file patterns for package selection and explicit listings.
#[derive(Debug)]
pub struct FilesMatcher {
    pub(super) rules: Vec<IgnoreRule>,
    pub(super) named_files: BTreeSet<String>,
}

impl FilesMatcher {
    /// Whether an explicit pattern matches this package-relative path.
    #[must_use]
    pub fn includes(&self, path: &str) -> bool {
        apply_rules(self.rules.iter(), true, path, false, None)
    }

    fn add_entry(&mut self, pkg_dir: &Path, entry: &str, directories: &mut HashMap<PathBuf, bool>) {
        let mut entry = entry
            .strip_prefix("./")
            .map_or_else(|| entry.to_string(), |entry| format!("/{entry}"));
        if entry.ends_with("/*") {
            entry.push('*');
        }
        let inverse = format!("!{entry}");
        let metadata = entry_metadata(pkg_dir, &entry, directories);
        match metadata {
            Some(metadata) if metadata.is_file() || metadata.is_symlink() => {
                self.add_named_file(&entry, IgnoreRule::parse(&inverse));
            }
            Some(metadata) if metadata.is_dir() => {
                self.rules.push(IgnoreRule::parse(&inverse));
                self.rules.push(IgnoreRule::parse(&format!("{inverse}/**")));
            }
            Some(_) => {}
            // npm-packlist treats every failed metadata lookup as a glob entry.
            None => self.rules.push(IgnoreRule::parse(&inverse)),
        }
    }

    fn add_named_file(&mut self, entry: &str, rule: IgnoreRule) {
        let path = normalize_field_path(entry);
        if !entry.starts_with('!') && rule_matches(&rule, &path, false, None) {
            self.named_files.insert(path);
        }
        self.rules.push(rule);
    }

    // Named files override the initial `*`, so only manifest exclusions need checking.
    fn apply_named_file_exclusions(&mut self) {
        let exclusions: Vec<_> = self.rules
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, rule)| !rule.negate)
            .collect();
        self.named_files.retain(|path| {
            let last_exclusion = exclusions
                .iter()
                .rev()
                .find(|(_, rule)| rule_matches(rule, path, false, None));
            let Some((index, _)) = last_exclusion else { return true };
            self.rules[index + 1..]
                .iter()
                .any(|rule| rule.negate && rule_matches(rule, path, false, None))
        });
    }
}

/// Match explicit listings in `files` without expanding directory entries.
/// Positive patterns are package-relative; exclusions may match at any depth.
pub fn build_files_matcher(entries: &[Value]) -> FilesMatcher {
    let rules = std::iter::once(IgnoreRule::parse("*"))
        .chain(
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(|entry| {
                    let inverse = if entry.starts_with('!') {
                        format!("!{entry}")
                    } else {
                        format!("!/{}", normalize_field_path(entry))
                    };
                    IgnoreRule::parse(&inverse)
                }),
        )
        .collect();
    FilesMatcher { rules, named_files: BTreeSet::new() }
}

/// Compile npm-packlist's manifest patterns without implicitly rooting them.
/// Existing files and symlinks are strict requirements; directories and globs
/// supply ordinary rules.
pub(super) fn compile_files_rules(pkg_dir: &Path, entries: &[Value]) -> FilesMatcher {
    let mut matcher =
        FilesMatcher { rules: vec![IgnoreRule::parse("*")], named_files: BTreeSet::new() };
    let mut directories = HashMap::new();
    for entry in entries.iter().filter_map(Value::as_str) {
        matcher.add_entry(pkg_dir, entry, &mut directories);
    }
    matcher.apply_named_file_exclusions();
    matcher
}

/// Normalize a manifest field path by stripping leading `./` and `/`.
pub(crate) fn normalize_field_path(path: &str) -> String {
    let trimmed = path.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}

/// Read entry metadata without escaping the package or following directory symlinks.
pub(super) fn entry_metadata(
    pkg_dir: &Path,
    entry: &str,
    directories: &mut HashMap<PathBuf, bool>,
) -> Option<fs::Metadata> {
    let relative = entry.trim_start_matches('!').trim_start_matches('/');
    if !relative.is_empty() && !crate::is_contained_field_path(relative) {
        return None;
    }
    let mut parent = PathBuf::new();
    for component in Path::new(relative)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .components()
    {
        parent.push(component);
        let is_directory = directories
            .entry(parent.clone())
            .or_insert_with(|| {
                fs::symlink_metadata(pkg_dir.join(&parent)).is_ok_and(|metadata| metadata.is_dir())
            });
        if !*is_directory {
            return None;
        }
    }
    fs::symlink_metadata(pkg_dir.join(relative)).ok()
}
