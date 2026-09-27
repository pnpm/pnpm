//! The manifest `files` allowlist: compile its entries into a matcher and
//! decide whether a packed path is included.

use ignore::gitignore::Gitignore;
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

/// Compile the `manifest.files` allowlist into a `Gitignore` matcher
/// rooted at `pkg_dir`. Returns `None` when no entries compile,
/// treating the field as absent.
pub fn build_files_matcher(pkg_dir: &Path, entries: &[Value]) -> Option<Gitignore> {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(pkg_dir);
    let mut added = 0;
    for entry in entries {
        let Some(raw) = entry.as_str() else { continue };
        let normalized = normalize_field_path(raw);
        if normalized.is_empty() {
            continue;
        }
        let pattern = anchor_files_entry(&normalized);
        if let Err(error) = builder.add_line(None, &pattern) {
            tracing::debug!(
                target: "pacquet::fs_packlist",
                ?pattern,
                ?error,
                "skipping invalid `files` entry",
            );
            continue;
        }
        added += 1;
    }
    if added == 0 {
        return None;
    }
    match builder.build() {
        Ok(gi) => Some(gi),
        Err(error) => {
            tracing::debug!(
                target: "pacquet::fs_packlist",
                ?error,
                "failed to build `files`-field matcher; treating field as absent",
            );
            None
        }
    }
}

/// The `files` entries that name an existing file rather than a glob, as
/// package-relative paths.
///
/// npm-packlist stats every entry and re-adds the files it names, which is
/// why `["**", "!dist", "dist/index.d.ts"]` still ships `dist/index.d.ts`:
/// the exclusion prunes the directory, not the file another entry names
/// (pnpm/pnpm#16213). A trailing slash makes an entry directory-only, so
/// `dist/index.d.ts/` does not name that file.
pub(super) fn named_file_entries(pkg_dir: &Path, entries: &[Value]) -> BTreeSet<String> {
    entries
        .iter()
        .filter_map(Value::as_str)
        .filter(|entry| !entry.starts_with('!') && !entry.ends_with('/') && !has_glob_syntax(entry))
        .map(normalize_field_path)
        .filter(|entry| !entry.is_empty() && pkg_dir.join(entry).is_file())
        .collect()
}

/// Whether a `files` entry carries glob syntax, and so cannot name one file.
fn has_glob_syntax(entry: &str) -> bool {
    entry
        .chars()
        .any(|character| matches!(character, '*' | '?' | '[' | ']' | '{' | '}'))
}

/// Anchor a `files` entry at the package root, leaving exclusions unanchored.
fn anchor_files_entry(pattern: &str) -> String {
    if pattern.starts_with('!') {
        return pattern.to_string();
    }
    format!("/{pattern}")
}

/// Whether `rel` matches the `files`-field allowlist, with exclusions on
/// ancestor directories taking precedence. The ancestor scan runs only
/// when some entry excludes anything, and only for paths no entry names:
/// a path the field names as a file survives the exclusion of the
/// directory holding it.
pub(super) fn files_field_includes(
    matcher: &Gitignore,
    rel: &str,
    named_files: &BTreeSet<String>,
) -> bool {
    if matcher.num_whitelists() > 0 && !named_files.contains(rel) {
        let path = Path::new(rel);
        if path
            .ancestors()
            .skip(1)
            .any(|ancestor| matcher.matched(ancestor, true).is_whitelist())
        {
            return false;
        }
    }
    matcher.matched_path_or_any_parents(rel, false).is_ignore()
}

/// Normalize a manifest field path by stripping leading `./` and `/`.
pub(super) fn normalize_field_path(path: &str) -> String {
    let trimmed = path.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}
