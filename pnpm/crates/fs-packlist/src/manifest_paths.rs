//! The paths a package manifest names: the `files` allowlist, the files a
//! tarball always carries whatever the manifest says, the cruft no tarball
//! ever carries, and the normalization a manifest field path needs to line up
//! with a packlist entry.

use ignore::gitignore::Gitignore;
use serde_json::Value;
use std::path::{Component, Path};

/// Case-insensitive prefix matches for files always-included at the
/// package root regardless of `.npmignore` / `files`. Mirrors
/// `npm-packlist`'s `alwaysIncluded` set.
const ALWAYS_INCLUDED_PREFIXES: &[&str] = &["readme", "license", "licence"];

/// Version-control directory names that exclude every file under
/// them at any depth. Drops VCS state from a published package
/// regardless of where in the tree it happens to sit, the same as
/// npm-packlist. Exact-segment match: a path with a literal segment
/// named `.git` / `.svn` / `.hg` / `CVS` is filtered, but a regular
/// file like `lib/foo.hg-stub` (basename `foo.hg-stub`, not `.hg`) is
/// not.
pub(super) const ALWAYS_EXCLUDED_DIR_SEGMENTS: &[&str] = &[".git", ".svn", ".hg", "CVS"];

/// Basenames always excluded regardless of where the file sits.
/// Matches npm-packlist's per-file cruft set: lockfiles for sibling
/// package managers, debug logs, OS junk, npm runtime config.
const ALWAYS_EXCLUDED_BASENAMES: &[&str] =
    &[".npmrc", "npm-debug.log", ".DS_Store", "package-lock.json", "yarn.lock", "pnpm-lock.yaml"];

/// Suffix-based always-excluded set, matching `npm-packlist`'s
/// `*.orig` exclusion family.
const ALWAYS_EXCLUDED_SUFFIXES: &[&str] = &[".orig"];

/// What a package's own manifest says should ship, beyond what the walk finds.
pub(super) struct FileSelection<'a> {
    /// The `files` allowlist, when the manifest declares one.
    pub(super) files_matcher: Option<&'a Gitignore>,
    pub(super) main_path: Option<&'a str>,
    pub(super) bin_paths: &'a [&'a str],
}

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
/// includes its contents recursively). It answers for the path itself
/// first, though, while npm-packlist's walker never descends into a
/// directory the `files` rules exclude, so
/// [`an_ancestor_is_excluded`] is consulted before it.
pub(super) fn files_field_includes(matcher: &Gitignore, rel: &str) -> bool {
    let matched = matcher.matched_path_or_any_parents(rel, false);
    if !matched.is_ignore() {
        return false;
    }
    // npm-packlist holds a `files` entry that names a file up for packing
    // whatever the entries around it say, so a `files` field that excludes a
    // directory still ships a file it names inside that directory.
    if matched
        .inner()
        .is_some_and(|glob| glob.original().trim_start_matches('/') == rel)
    {
        return true;
    }
    !an_ancestor_is_excluded(matcher, rel)
}

/// Whether a `files`-field exclusion names one of `rel`'s ancestor
/// directories. npm-packlist's walker prunes a directory the `files` rules
/// exclude without descending into it, so a broad entry that matches every
/// file does not put back the files the exclusion reaches.
fn an_ancestor_is_excluded(matcher: &Gitignore, rel: &str) -> bool {
    rel.match_indices('/')
        .any(|(before, _)| {
            matcher
                .matched(&rel[..before], true)
                .is_whitelist()
        })
}

pub(super) fn is_always_included_at_root(rel: &str) -> bool {
    // Only files at the root carry the always-include semantics; a
    // `LICENSE` deep in a subtree follows the same `.npmignore` /
    // `files` rules as any other file. Matches npm-packlist's
    // root-only treatment of the README/LICENSE/etc. set.
    if rel.contains('/') {
        return false;
    }
    let lower = rel.to_ascii_lowercase();
    if lower == "package.json" {
        return true;
    }
    ALWAYS_INCLUDED_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

pub(super) fn is_main_or_bin(rel: &str, main: Option<&str>, bins: &[&str]) -> bool {
    if let Some(main) = main
        && normalize_field_path(main) == rel
    {
        return true;
    }
    bins.iter()
        .any(|bin| normalize_field_path(bin) == rel)
}

pub(super) fn should_always_exclude(rel: &str) -> bool {
    let basename = rel.rsplit('/').next().unwrap_or(rel);
    // Basename-cruft check: per-file entries (`.npmrc`, lockfiles,
    // debug logs, OS junk) are excluded at any depth.
    if ALWAYS_EXCLUDED_BASENAMES.contains(&basename) {
        return true;
    }
    // VCS dir check: a path is excluded if any segment is literally
    // `.git` / `.svn` / `.hg` / `CVS`. Exact-segment match (not
    // prefix) so a regular file `lib/foo.hg-stub` isn't accidentally
    // dropped just because its basename mentions `.hg`.
    if rel
        .split('/')
        .any(|seg| ALWAYS_EXCLUDED_DIR_SEGMENTS.contains(&seg))
    {
        return true;
    }
    ALWAYS_EXCLUDED_SUFFIXES
        .iter()
        .any(|suffix| basename.ends_with(suffix))
}

/// Strip a leading `./` and any leading slashes from `path` so manifest
/// field entries match the forward-slash relative form `packlist`
/// produces. Mirrors `npm-packlist`'s normalization step.
pub(super) fn normalize_field_path(path: &str) -> String {
    let trimmed = path.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}

/// Whether a [`normalize_field_path`]-ed `main` / `bin` value stays inside
/// the package: non-empty, only normal path components (no `..`, root, or
/// drive/UNC prefix), and no backslash (a separator on Windows, where the
/// git fetcher may import a package). The packlist feeds both `pack` and
/// the git fetcher on attacker-controlled manifests, so an escaping field
/// must never be force-included — a `main: "../secret"` would otherwise be
/// read into the tarball / CAS.
pub(super) fn is_contained_field_path(normalized: &str) -> bool {
    !normalized.is_empty()
        && !normalized.contains('\\')
        && Path::new(normalized)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}
