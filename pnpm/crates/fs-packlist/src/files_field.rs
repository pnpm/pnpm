//! The manifest `files` allowlist: compile its entries into a matcher and
//! decide whether a packed path is included.

use ignore::gitignore::Gitignore;
use serde_json::Value;
use std::path::Path;

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

/// Anchor a `files` entry at the package root, leaving exclusions unanchored.
fn anchor_files_entry(pattern: &str) -> String {
    if pattern.starts_with('!') {
        return pattern.to_string();
    }
    format!("/{pattern}")
}

/// Whether `rel` matches the `files`-field allowlist, with exclusions on
/// ancestor directories taking precedence.
pub(super) fn files_field_includes(matcher: &Gitignore, rel: &str) -> bool {
    let path = Path::new(rel);
    if path
        .ancestors()
        .skip(1)
        .any(|ancestor| matcher.matched(ancestor, true).is_whitelist())
    {
        return false;
    }
    matcher.matched_path_or_any_parents(rel, false).is_ignore()
}

/// Normalize a manifest field path by stripping leading `./` and `/`.
pub(super) fn normalize_field_path(path: &str) -> String {
    let trimmed = path.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}
