//! Warns when a tarball is about to carry dotenv files that the manifest's
//! `files` field never named. npm-compatible file selection packs them
//! whenever no ignore file excludes them, so they leak local secrets on
//! publish. Naming a file in `files` is how a package ships one on purpose.

use super::{LogEvent, LogLevel, Path, PathBuf, Reporter, Value};
use pnpm_reporter::PnpmLog;

/// Dotenv files that document variables rather than hold their values.
const DOTENV_TEMPLATES: &[&str] = &[".env.example", ".env.sample", ".env.template"];

const GLOB_CHARS: &[char] = &['*', '?', '[', '{'];

/// Emit one warning listing every packed dotenv file that no `files`
/// entry names.
pub(super) fn warn_about_unlisted_dotenv_files<Reporter: self::Reporter>(
    project_dir: &Path,
    publish_manifest: &Value,
    files_map: &indexmap::IndexMap<String, PathBuf>,
) {
    let files_entries: Vec<&str> = publish_manifest
        .get("files")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    let unlisted: Vec<&str> = files_map
        .keys()
        .filter_map(|name| name.strip_prefix("package/"))
        .filter(|path| is_dotenv_file(path) && !is_named_in_files(path, &files_entries))
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
        prefix: project_dir.to_string_lossy().into_owned(),
    }));
}

/// `.env` and `.env.<suffix>` at any depth, except the committed templates.
pub(super) fn is_dotenv_file(path: &str) -> bool {
    let basename = basename(path);
    basename == ".env" || (basename.starts_with(".env.") && !DOTENV_TEMPLATES.contains(&basename))
}

/// Whether a `files` entry names `path` itself, or is a glob whose last
/// segment targets dotenv files (`**/.env`, `.env*`). A directory entry
/// such as `dist` that merely contains the file does not count.
pub(super) fn is_named_in_files(path: &str, files_entries: &[&str]) -> bool {
    files_entries
        .iter()
        .any(|entry| {
            let entry = entry.trim_start_matches("./").trim_start_matches('/');
            entry == path || (entry.contains(GLOB_CHARS) && basename(entry).starts_with(".env"))
        })
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
