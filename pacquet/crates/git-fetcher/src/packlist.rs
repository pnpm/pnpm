//! Decide which files inside an extracted git-hosted package end up in
//! the CAS. Port of [`npm-packlist`](https://github.com/npm/npm-packlist)
//! and pnpm's
//! [`fs/packlist`](https://github.com/pnpm/pnpm/blob/94240bc046/fs/packlist/src/index.ts).
//!
//! The algorithm has four passes:
//!
//! 1. **Honor `.npmignore` and `.gitignore`** while walking. The
//!    `ignore::WalkBuilder` does per-directory inheritance: a
//!    `.npmignore` in `lib/` applies to `lib/**` only, while the
//!    package's root `.gitignore` applies to the whole tree.
//! 2. **Apply the `files` field allowlist** on top of the walk's
//!    output: when the manifest sets `files: ["dist/**"]`, drop
//!    anything outside that set (except the always-included files
//!    handled in pass 3).
//! 3. **Always-include** the standard files: `package.json`,
//!    `README*` / `LICEN[SC]E*` / `CHANGES*` / `CHANGELOG*` /
//!    `HISTORY*` / `NOTICE*` at the root, plus the paths declared in
//!    `main` / `bin`. These survive `.npmignore` rejection and the
//!    `files`-field filter.
//! 4. **`bundleDependencies` closure**: starting from the names in
//!    `manifest.bundleDependencies` (or the legacy
//!    `bundledDependencies`), transitively include every reachable
//!    dependency. A bundled package pulls in its own `dependencies`
//!    and `optionalDependencies` too, so the whole closure ships.
//!    Each name is resolved with the node module-resolution walk-up
//!    (nested `node_modules/` first, then ancestor `node_modules/`),
//!    which is what lets a hoisted transitive dep at the root
//!    `node_modules/` be found and spliced in under its real path.
//!    Port of [`npm-bundled`](https://github.com/npm/npm-bundled).
//!
//! Two intentional divergences from upstream:
//!
//! - The `ignore` crate combines `.npmignore` and `.gitignore` rules
//!   when both files exist in the same directory; npm-packlist would
//!   use only `.npmignore`. The combined-rules outcome is the same
//!   for the common case (a `.npmignore` that's a strict superset of
//!   `.gitignore`); the divergence shows up only when `.npmignore`
//!   explicitly *includes* a path `.gitignore` excludes, which is a
//!   rare configuration in the wild. Documented gap; revisit if a
//!   real package surfaces it.
//! - `.git/info/exclude` and global `~/.gitignore` are NOT honored —
//!   only the in-tree `.gitignore` / `.npmignore` files. The fetcher
//!   imports a clean tarball / git checkout, not a user's working
//!   tree, so the global-state ignores are wrong by construction.

use crate::error::PacklistError;
use ignore::{WalkBuilder, gitignore::Gitignore};
use pacquet_package_manifest::safe_read_package_json_from_dir;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

/// Cap on `bundleDependencies` closure depth. Real packages bundle
/// at most a handful of levels (most published packages bundle zero;
/// the rare ones bundle one or two). The visited-set already makes
/// the walk terminate; this cap is belt-and-braces against a
/// pathological tree that keeps resolving fresh canonical paths
/// (e.g. a deep chain of `dependencies` that never repeats).
const MAX_BUNDLE_DEPTH: u32 = 32;

/// Case-insensitive prefix matches for files always-included at the
/// package root regardless of `.npmignore` / `files`. Mirrors
/// `npm-packlist`'s `alwaysIncluded` plus pnpm's pattern set at
/// [`fs/packlist/src/index.ts:13`](https://github.com/pnpm/pnpm/blob/94240bc046/fs/packlist/src/index.ts#L13).
const ALWAYS_INCLUDED_PREFIXES: &[&str] =
    &["readme", "license", "licence", "changes", "changelog", "history", "notice"];

/// Version-control directory names that exclude every file under
/// them at any depth. Matches the upstream behavior of dropping VCS
/// state from a published package regardless of where in the tree it
/// happens to sit. Exact-segment match: a path with a literal segment
/// named `.git` / `.svn` / `.hg` / `CVS` is filtered, but a regular
/// file like `lib/foo.hg-stub` (basename `foo.hg-stub`, not `.hg`) is
/// not.
const ALWAYS_EXCLUDED_DIR_SEGMENTS: &[&str] = &[".git", ".svn", ".hg", "CVS"];

/// Basenames always excluded regardless of where the file sits.
/// Matches npm-packlist's per-file cruft set: lockfiles for sibling
/// package managers, debug logs, OS junk, npm runtime config.
const ALWAYS_EXCLUDED_BASENAMES: &[&str] =
    &[".npmrc", "npm-debug.log", ".DS_Store", "package-lock.json", "yarn.lock", "pnpm-lock.yaml"];

/// Suffix-based always-excluded set, matching `npm-packlist`'s
/// `*.orig` exclusion family.
const ALWAYS_EXCLUDED_SUFFIXES: &[&str] = &[".orig"];

/// Walk `pkg_dir` and return forward-slash relative paths for every
/// file the published tarball should contain. Mirrors the return
/// shape of `packlist()` at
/// [`fs/packlist/src/index.ts:24-29`](https://github.com/pnpm/pnpm/blob/94240bc046/fs/packlist/src/index.ts#L24-L29)
/// (paths relative to `pkg_dir`, no leading `./`).
pub fn packlist(pkg_dir: &Path, manifest: &Value) -> Result<Vec<String>, PacklistError> {
    let mut out: BTreeSet<String> = collect_own_files(pkg_dir, manifest)?;
    collect_bundled_files(pkg_dir, manifest, &mut out)?;
    Ok(out.into_iter().collect())
}

/// One unit of `bundleDependencies`-closure work: resolve `name`
/// starting the node module-resolution walk-up at `from_dir`, then
/// splice the resolved package's files into the output.
struct BundleTask {
    name: String,
    from_dir: PathBuf,
    depth: u32,
}

/// Build the `bundleDependencies` closure for `root` and splice each
/// bundled package's files into `out` under the package's real path
/// relative to `root` (e.g. `node_modules/<name>/...`).
///
/// Mirrors [`npm-bundled`](https://github.com/npm/npm-bundled): seed
/// from the root manifest's bundle list, then transitively pull in
/// every reachable dependency. Once a package is bundled, its own
/// `dependencies` and `optionalDependencies` are bundled too — that
/// is how the closure reaches a hoisted transitive dep sitting at the
/// root `node_modules/`. `devDependencies` are never followed.
///
/// The `visited` set is keyed on the canonicalised resolved directory,
/// so a diamond (two bundled deps sharing a transitive dep) processes
/// the shared package once and a `dependencies` cycle terminates
/// instead of looping forever.
fn collect_bundled_files(
    root: &Path,
    root_manifest: &Value,
    out: &mut BTreeSet<String>,
) -> Result<(), PacklistError> {
    // Canonical form of the package root, used to reject any bundled
    // dependency whose real path escapes the tree (see the symlink check
    // in the loop below). `None` if `root` itself can't be canonicalised,
    // in which case the escape check falls back to a lexical comparison.
    let canonical_root = fs::canonicalize(root).ok();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<BundleTask> = root_bundle_dep_names(root_manifest)
        .into_iter()
        .map(|name| BundleTask { name, from_dir: root.to_path_buf(), depth: 0 })
        .collect();

    while let Some(task) = queue.pop_front() {
        if task.depth > MAX_BUNDLE_DEPTH {
            tracing::warn!(
                target: "pacquet::git_fetcher::packlist",
                bundle_name = %task.name,
                depth = task.depth,
                "bundleDependencies closure exceeded MAX_BUNDLE_DEPTH; refusing to descend further",
            );
            continue;
        }
        // Defense-in-depth: a malicious manifest could carry
        // `bundleDependencies: ["../../etc"]` (or an absolute path).
        // Reject anything that's not a single safe segment before it
        // reaches the join in `resolve_bundled_dependency`.
        if !is_safe_bundle_name(&task.name) {
            tracing::warn!(
                target: "pacquet::git_fetcher::packlist",
                bundle_name = %task.name,
                "rejecting bundleDependencies entry that is not a single path segment",
            );
            continue;
        }
        let Some(dep_dir) = resolve_bundled_dependency(&task.name, &task.from_dir, root) else {
            tracing::debug!(
                target: "pacquet::git_fetcher::packlist",
                bundle_name = %task.name,
                from_dir = %task.from_dir.display(),
                "bundleDependencies entry not resolvable under node_modules/; skipping",
            );
            continue;
        };
        // `fs::canonicalize` resolves symlinks, giving both the dedup
        // key (a symlink loop shows up as an already-visited path) and
        // the real target for the escape check below. `None` on failure
        // (e.g. permission denied); dedup then degrades to the raw path
        // and the escape check to a lexical comparison.
        let canonical_dep = fs::canonicalize(&dep_dir).ok();
        // `is_safe_bundle_name` only screens the name; a
        // `node_modules/<name>` symlink pointing at a sibling or an
        // absolute host path passes that yet resolves outside the tree.
        // Walking it would splice host files into the published set, so
        // refuse anything whose real path is not under the root. The
        // fetcher imports untrusted git-hosted packages, so this matters.
        let escapes = match (&canonical_root, &canonical_dep) {
            (Some(root), Some(dep)) => dep.strip_prefix(root).is_err(),
            // Root resolved but the dependency's real path didn't: we
            // can't prove it stays inside the tree, so fail closed. A
            // genuine dependency always canonicalises here —
            // `resolve_bundled_dependency` already stat'd its
            // `package.json` through the same path.
            (Some(_), None) => true,
            // Root itself won't canonicalise (pathological): fall back
            // to a best-effort lexical check.
            _ => dep_dir.strip_prefix(root).is_err(),
        };
        if escapes {
            tracing::warn!(
                target: "pacquet::git_fetcher::packlist",
                bundle_name = %task.name,
                dep_dir = %dep_dir.display(),
                "bundled dependency resolves outside the package tree; refusing",
            );
            continue;
        }
        if !visited.insert(canonical_dep.unwrap_or_else(|| dep_dir.clone())) {
            continue;
        }
        let prefix = relative_forward_slash(root, &dep_dir);
        let dep_manifest = safe_read_package_json_from_dir(&dep_dir)
            .ok()
            .flatten()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        for rel in collect_own_files(&dep_dir, &dep_manifest)? {
            out.insert(format!("{prefix}/{rel}"));
        }
        for name in nested_bundle_dep_names(&dep_manifest) {
            queue.push_back(BundleTask { name, from_dir: dep_dir.clone(), depth: task.depth + 1 });
        }
    }
    Ok(())
}

/// Resolve a bundled dependency `name` to its directory using the
/// node module-resolution walk-up: check `from_dir/node_modules/name`,
/// then climb to each ancestor's `node_modules/`, stopping at `root`.
/// Returns the first directory that contains a `package.json`, or
/// `None` if the name resolves nowhere within the package tree.
///
/// Climbing past `root` is refused so a hoisted dep always resolves to
/// the package being packed rather than to a sibling on the host.
fn resolve_bundled_dependency(name: &str, from_dir: &Path, root: &Path) -> Option<PathBuf> {
    let mut current = from_dir.to_path_buf();
    loop {
        let candidate = current.join("node_modules").join(name);
        if candidate.join("package.json").is_file() {
            return Some(candidate);
        }
        if current == root {
            return None;
        }
        let parent = current.parent()?;
        if parent == current {
            return None;
        }
        current = parent.to_path_buf();
    }
}

/// Collect the forward-slash relative paths for a single package's own
/// files — the `.npmignore` / `.gitignore` walk, the `files`-field
/// allowlist, and the always-included / `main` / `bin` force-includes.
/// This is the per-package packlist with no `bundleDependencies`
/// traversal; [`collect_bundled_files`] layers the closure on top.
fn collect_own_files(pkg_dir: &Path, manifest: &Value) -> Result<BTreeSet<String>, PacklistError> {
    let files_field = manifest.get("files").and_then(Value::as_array);
    let files_matcher: Option<Gitignore> =
        files_field.and_then(|arr| build_files_matcher(pkg_dir, arr));
    let main_path = manifest.get("main").and_then(Value::as_str);
    let bin_paths: Vec<&str> = manifest
        .get("bin")
        .map(|bin| match bin {
            Value::String(s) => vec![s.as_str()],
            Value::Object(map) => map.values().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();

    let mut out: BTreeSet<String> = BTreeSet::new();

    // Pass 1: walk with `.gitignore` / `.npmignore` filtering.
    // `standard_filters(false)` turns off `ignore`'s opinionated
    // defaults (hidden-file skip, `.git`-dir skip, etc.) so we control
    // every filter explicitly. `git_ignore(true)` /
    // `add_custom_ignore_filename(".npmignore")` enable the two ignore
    // file sources; `require_git(false)` makes `ignore` honor
    // `.gitignore` even though a git-hosted snapshot's `.git/` has
    // already been deleted by [`crate::GitFetcher`] before this point.
    let mut builder = WalkBuilder::new(pkg_dir);
    builder
        .standard_filters(false)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(false)
        .git_global(false)
        .require_git(false)
        // Don't search parent directories of `pkg_dir` for ignore
        // files: the packlist must depend only on the contents of the
        // package directory itself. Otherwise a `.gitignore` in the
        // workspace root above a git-hosted snapshot's working copy
        // would leak into the published file set.
        .parents(false)
        .add_custom_ignore_filename(".npmignore");

    for entry in builder.build() {
        let entry = entry.map_err(|err| io_error(pkg_dir, into_io(err)))?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = relative_forward_slash(pkg_dir, entry.path());
        if should_always_exclude(&rel) {
            continue;
        }
        // `node_modules/` contents are bundled by
        // `collect_bundled_files`, never via this general walk.
        // Without this gate a manifest that publishes a stray
        // `node_modules/something` would slip through.
        if rel.starts_with("node_modules/") || rel == "node_modules" {
            continue;
        }
        if let Some(matcher) = &files_matcher
            && !files_field_includes(matcher, &rel)
            && !is_always_included_at_root(&rel)
            && !is_main_or_bin(&rel, main_path, &bin_paths)
        {
            continue;
        }
        out.insert(rel);
    }

    // Pass 2: scan the root for always-included names (README, LICENSE,
    // etc.) that `.npmignore` might have removed from pass 1. npm-
    // packlist guarantees these survive `.npmignore`.
    let root_entries = fs::read_dir(pkg_dir)
        .map_err(|source| PacklistError::Io { pkg_dir: pkg_dir.display().to_string(), source })?;
    for entry in root_entries {
        let entry = entry.map_err(|source| PacklistError::Io {
            pkg_dir: pkg_dir.display().to_string(),
            source,
        })?;
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if should_always_exclude(&name) {
            continue;
        }
        if is_always_included_at_root(&name) {
            out.insert(name);
        }
    }

    // Pass 3: force-include `main` / `bin` paths, which always ship
    // regardless of `.npmignore`. (`files`-field rejection is already
    // overridden in pass 1.) Still consult `should_always_exclude`
    // first so the always-excluded set wins over manifest fields;
    // npm-packlist does the same and emits no warning, so we stay
    // silent too (a `tracing::debug!` would be lost in install logs).
    if let Some(main) = main_path {
        let main_norm = normalize_field_path(main);
        if !main_norm.is_empty()
            && !should_always_exclude(&main_norm)
            && pkg_dir.join(&main_norm).is_file()
        {
            out.insert(main_norm);
        }
    }
    for bin in &bin_paths {
        let bin_norm = normalize_field_path(bin);
        if !bin_norm.is_empty()
            && !should_always_exclude(&bin_norm)
            && pkg_dir.join(&bin_norm).is_file()
        {
            out.insert(bin_norm);
        }
    }

    Ok(out)
}

/// Compile the `manifest.files` allowlist into a single `Gitignore`
/// matcher rooted at `pkg_dir`. Returns `None` when no entries
/// compile (e.g., the field was present but every entry was empty or
/// malformed) so the caller treats the absence as "include
/// everything", matching upstream's behavior for an unset / empty
/// `files`. Lines that fail to parse are dropped with a
/// `tracing::debug!` — npm-packlist tolerates bad globs the same way
/// (a bad pattern just doesn't match anything).
fn build_files_matcher(pkg_dir: &Path, entries: &[Value]) -> Option<Gitignore> {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(pkg_dir);
    let mut added = 0;
    for entry in entries {
        let Some(raw) = entry.as_str() else { continue };
        let pattern = normalize_field_path(raw);
        if pattern.is_empty() {
            continue;
        }
        if let Err(error) = builder.add_line(None, &pattern) {
            tracing::debug!(
                target: "pacquet::git_fetcher::packlist",
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
                target: "pacquet::git_fetcher::packlist",
                ?error,
                "failed to build `files`-field matcher; treating field as absent",
            );
            None
        }
    }
}

/// `true` when `rel` matches the `files`-field allowlist. The matcher
/// was built with the `files` entries as gitignore-style include
/// patterns.
///
/// `Gitignore::matched_path_or_any_parents` walks the path's ancestor
/// chain and returns `Ignore` when any segment matches — exactly the
/// behavior npm-packlist's `files`-field needs (a directory pattern
/// includes its contents recursively).
fn files_field_includes(matcher: &Gitignore, rel: &str) -> bool {
    matcher.matched_path_or_any_parents(rel, false).is_ignore()
}

fn is_always_included_at_root(rel: &str) -> bool {
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
    ALWAYS_INCLUDED_PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
}

fn is_main_or_bin(rel: &str, main: Option<&str>, bins: &[&str]) -> bool {
    if let Some(main) = main
        && normalize_field_path(main) == rel
    {
        return true;
    }
    bins.iter().any(|bin| normalize_field_path(bin) == rel)
}

fn should_always_exclude(rel: &str) -> bool {
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
    if rel.split('/').any(|seg| ALWAYS_EXCLUDED_DIR_SEGMENTS.contains(&seg)) {
        return true;
    }
    ALWAYS_EXCLUDED_SUFFIXES.iter().any(|suffix| basename.ends_with(suffix))
}

/// True when `name` is a safe `bundleDependencies` entry — the join
/// `pkg_dir/node_modules/<name>` stays inside `pkg_dir/node_modules`.
///
/// Rejects parent-dir components, root, and drive prefixes. Accepts
/// scoped names like `@scope/foo`: those legitimately carry a slash
/// and resolve to `pkg_dir/node_modules/@scope/foo`, which is still
/// inside the package tree. Same component-based discipline
/// `cas_io::join_checked` uses for tarball entries.
fn is_safe_bundle_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let path = std::path::Path::new(name);
    if path.is_absolute() {
        return false;
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return false;
            }
            // `.` components are stripped silently — `./foo` resolves
            // the same as `foo` on every platform.
            std::path::Component::CurDir => {}
        }
    }
    true
}

/// Seed names for the bundle closure: the root manifest's
/// `bundleDependencies` (or the legacy `bundledDependencies`). Both
/// spellings appear in real published packages; npm-packlist accepts
/// either.
fn root_bundle_dep_names(manifest: &Value) -> Vec<String> {
    let raw = manifest.get("bundleDependencies").or_else(|| manifest.get("bundledDependencies"));
    let Some(raw) = raw else { return Vec::new() };
    match raw {
        Value::Array(arr) => arr.iter().filter_map(Value::as_str).map(String::from).collect(),
        Value::Bool(true) => {
            // `bundleDependencies: true` means "bundle every entry in
            // `dependencies`". Rare but supported by npm. Materialize
            // the keys from the dependencies map.
            manifest
                .get("dependencies")
                .and_then(Value::as_object)
                .map(|map| map.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Names a bundled package pulls into the closure: every key in its
/// own `dependencies` and `optionalDependencies`. A bundled package
/// ships its whole runtime closure, so these are followed regardless
/// of whether the nested package declares its own `bundleDependencies`
/// (already-bundled packages don't re-gate their deps). `peer`- and
/// `dev`-dependencies are deliberately excluded — they are not part of
/// the published closure. Mirrors `npm-bundled`'s `getDeps`.
fn nested_bundle_dep_names(manifest: &Value) -> Vec<String> {
    let mut names = Vec::new();
    for field in ["dependencies", "optionalDependencies"] {
        if let Some(map) = manifest.get(field).and_then(Value::as_object) {
            names.extend(map.keys().cloned());
        }
    }
    names
}

fn relative_forward_slash(root: &Path, full: &Path) -> String {
    let rel = full.strip_prefix(root).unwrap_or(full);
    let mut buf = PathBuf::from(rel).into_os_string().to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR != '/' {
        buf = buf.replace(std::path::MAIN_SEPARATOR, "/");
    }
    buf
}

/// Strip a leading `./` and any leading slashes from `path` so manifest
/// field entries match the forward-slash relative form `packlist`
/// produces. Mirrors `npm-packlist`'s normalization step.
fn normalize_field_path(path: &str) -> String {
    let trimmed = path.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}

fn io_error(pkg_dir: &Path, source: std::io::Error) -> PacklistError {
    PacklistError::Io { pkg_dir: pkg_dir.display().to_string(), source }
}

fn into_io(err: ignore::Error) -> std::io::Error {
    err.into_io_error()
        .unwrap_or_else(|| std::io::Error::other("ignore walker produced a non-io error"))
}

#[cfg(test)]
mod tests;
