//! Compile the manifest allowlist into the walk's ignore rules.

use super::{IgnoreRule, apply_rules};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};

/// A manifest `files` allowlist, using the same rules as the package walk.
#[derive(Debug)]
pub struct FilesMatcher {
    pub(super) rules: Vec<IgnoreRule>,
    pub(super) named_files: BTreeSet<String>,
}

impl FilesMatcher {
    /// Whether the allowlist includes this package-relative path.
    #[must_use]
    pub fn includes(&self, path: &str) -> bool {
        if self.named_files.contains(path) {
            return true;
        }
        let ancestors_included = Path::new(path)
            .ancestors()
            .skip(1)
            .all(|ancestor| {
                ancestor.as_os_str().is_empty()
                    || apply_rules(self.rules.iter(), true, &ancestor.to_string_lossy(), true, None)
            });
        ancestors_included && apply_rules(self.rules.iter(), true, path, false, None)
    }

    fn add_entry(&mut self, pkg_dir: &Path, entry: &str) {
        let mut entry = entry
            .strip_prefix("./")
            .unwrap_or(entry)
            .to_string();
        if entry.ends_with("/*") {
            entry.push('*');
        }
        let inverse = format!("!{entry}");
        let metadata = fs::symlink_metadata(pkg_dir.join(entry.trim_start_matches(['!', '/'])));
        match metadata {
            Ok(metadata) if metadata.is_file() || metadata.is_symlink() => {
                if !entry.starts_with('!') {
                    self.named_files.insert(normalize_field_path(&entry));
                }
                self.rules.push(IgnoreRule::parse(&inverse));
            }
            Ok(metadata) if metadata.is_dir() => {
                self.rules.push(IgnoreRule::parse(&inverse));
                self.rules.push(IgnoreRule::parse(&format!("{inverse}/**")));
            }
            Ok(_) => {}
            // npm-packlist treats every failed metadata lookup as a glob entry.
            Err(_) => self.rules.push(IgnoreRule::parse(&inverse)),
        }
    }
}

/// Match explicit listings in `files`. Positive patterns are rooted at
/// `pkg_dir`; exclusions may match at any depth.
pub fn build_files_matcher(pkg_dir: &Path, entries: &[Value]) -> FilesMatcher {
    let entries: Vec<Value> = entries
        .iter()
        .filter_map(Value::as_str)
        .map(|entry| {
            if entry.starts_with('!') {
                Value::String(entry.to_string())
            } else {
                Value::String(format!("/{}", normalize_field_path(entry)))
            }
        })
        .collect();
    compile_files_rules(pkg_dir, &entries)
}

/// Existing files and symlinks are strict requirements; directories and globs
/// supply ordinary rules, as in npm-packlist's manifest rule synthesis.
pub(super) fn compile_files_rules(pkg_dir: &Path, entries: &[Value]) -> FilesMatcher {
    let mut matcher =
        FilesMatcher { rules: vec![IgnoreRule::parse("*")], named_files: BTreeSet::new() };
    for entry in entries.iter().filter_map(Value::as_str) {
        matcher.add_entry(pkg_dir, entry);
    }
    matcher.named_files.retain(|path| apply_rules(matcher.rules.iter(), true, path, false, None));
    matcher
}

/// Normalize a manifest field path by stripping leading `./` and `/`.
pub(crate) fn normalize_field_path(path: &str) -> String {
    let trimmed = path.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}
