//! Warns when a tarball is about to carry dotenv files that the manifest's
//! `files` field never named. npm-compatible file selection packs them
//! whenever no ignore file excludes them, so they leak local secrets on
//! publish. Naming a file in `files` is how a package ships one on purpose.

use super::{LogEvent, LogLevel, Path, PathBuf, Reporter, Value};
use pnpm_fs_packlist::build_files_matcher;
use pnpm_reporter::PnpmLog;

/// Dotenv files that document variables rather than hold their values.
const DOTENV_TEMPLATES: &[&str] = &[".env.example", ".env.sample", ".env.template"];

/// Emit one warning listing every packed dotenv file that no `files`
/// entry names.
pub(super) fn warn_about_unlisted_dotenv_files<Reporter: self::Reporter>(
    pkg_dir: &Path,
    publish_manifest: &Value,
    files_map: &indexmap::IndexMap<String, PathBuf>,
) {
    let files_field = publish_manifest
        .get("files")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let is_listed = named_in_files(pkg_dir, files_field);
    let unlisted: Vec<&str> = files_map
        .keys()
        .filter_map(|name| name.strip_prefix("package/"))
        .filter(|path| is_dotenv_file(path) && !is_listed(path))
        .collect();
    if unlisted.is_empty() {
        return;
    }
    let listing = unlisted.join("\n  ");
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!(
            "The package tarball includes dotenv files that may contain secrets:\n  {listing}\n\
             Exclude them in .npmignore (or .gitignore if the package has no .npmignore), \
             or list them in the \"files\" field of package.json to publish them on purpose. \
             pnpm 13 will refuse to pack dotenv files that \"files\" does not list.",
        ),
        prefix: pkg_dir.to_string_lossy().into_owned(),
    }));
}

/// `.env` and `.env.<suffix>` at any depth, except the committed templates.
pub(super) fn is_dotenv_file(path: &str) -> bool {
    let basename = basename(path);
    basename == ".env" || (basename.starts_with(".env.") && !DOTENV_TEMPLATES.contains(&basename))
}

/// Whether a `files` entry whose last segment starts with `.env` matches
/// the packed path, with the packlist's own `files` semantics. A
/// directory entry such as `dist` that merely contains the file does not
/// count, and neither does a dotenv glob rooted elsewhere.
pub(super) fn named_in_files(pkg_dir: &Path, files_field: &[Value]) -> impl Fn(&str) -> bool {
    let dotenv_entries: Vec<Value> = files_field
        .iter()
        .filter(|entry| {
            entry
                .as_str()
                .is_some_and(|entry| basename(entry.trim_end_matches('/')).starts_with(".env"))
        })
        .cloned()
        .collect();
    let matcher = build_files_matcher(pkg_dir, &dotenv_entries);
    move |path| {
        matcher
            .as_ref()
            .is_some_and(|matcher| matcher.matched(path, false).is_ignore())
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
