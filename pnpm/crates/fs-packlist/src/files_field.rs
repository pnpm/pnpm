//! The manifest `files` allowlist: compile its entries into a matcher and
//! decide whether a packed path is included.

use ignore::gitignore::Gitignore;
use serde_json::Value;
use std::path::Path;

/// Compile the `manifest.files` allowlist into a single `Gitignore`
/// matcher rooted at `pkg_dir`. Returns `None` when no entries
/// compile (e.g., the field was present but every entry was empty or
/// malformed) so the caller treats the absence as "include
/// everything", the same as an unset / empty `files`. Lines that fail
/// to parse are dropped with a `tracing::debug!` — npm-packlist
/// tolerates bad globs the same way (a bad pattern just doesn't match
/// anything).
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

/// Anchor a [`normalize_field_path`]-ed `files` entry at the package
/// root — the matcher is rooted there, so the leading slash is what
/// binds the pattern to it. An exclusion is left unanchored, matching
/// how npm-packlist hands a negated entry to `ignore-walk`.
fn anchor_files_entry(pattern: &str) -> String {
    if pattern.starts_with('!') {
        return pattern.to_string();
    }
    format!("/{pattern}")
}

/// `true` when `rel` matches the `files`-field allowlist. The matcher
/// was built with the `files` entries as gitignore-style include
/// patterns.
///
/// `Gitignore::matched_path_or_any_parents` walks the path's ancestor
/// chain and returns `Ignore` when any segment matches — exactly the
/// behavior npm-packlist's `files`-field needs (a directory pattern
/// includes its contents recursively).
///
/// An exclusion naming an ancestor directory (`!**/test`) wins over
/// any include match on the file itself: `matched_path_or_any_parents`
/// answers for the deepest path first, so without the ancestor scan a
/// broad include (`**`) would keep the file even though npm-packlist,
/// which decides directories before descending into them, drops the
/// whole subtree. Like git, a file under an excluded directory cannot
/// be re-included.
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

/// Strip a leading `./` and any leading slashes from `path` so manifest
/// field entries match the forward-slash relative form `packlist`
/// produces. Mirrors `npm-packlist`'s normalization step.
pub(super) fn normalize_field_path(path: &str) -> String {
    let trimmed = path.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}
