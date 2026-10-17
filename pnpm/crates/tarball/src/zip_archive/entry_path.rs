//! Validation and cleaning of one zip entry's path.

use super::{Component, Cursor, IgnoreEntryFilter, TarballError};

/// The canonical, prefix-stripped path of one zip entry, or `None` when the
/// entry carries no payload to store (a directory, the prefix directory
/// itself, or an ignored file).
pub(super) fn zip_entry_path(
    entry: &zip::read::ZipFile<'_, Cursor<Vec<u8>>>,
    package_url: &str,
    basename_prefix: Option<&str>,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<Option<String>, TarballError> {
    // Validate the path *before* the `is_dir()` early-skip so an
    // archive carrying a directory entry like `../evil/` still
    // surfaces [`TarballError::PathTraversal`] rather than being
    // silently dropped. Pacquet wouldn't write that directory
    // either way (only file entries take the CAS write path
    // below), but rejecting outright keeps the "no unsafe entry
    // accepted" contract intact for tooling that inspects the
    // error code.
    let raw_name = entry.name().to_string();
    let traversal = |entry_path: String| TarballError::PathTraversal {
        url: package_url.to_string(),
        entry_path,
        reason: "zip entry path is absolute or escapes the archive root",
    };
    // [`zip::read::ZipFile::enclosed_name`] returns `None` for
    // absolute paths and any path with a `..` component — a
    // single check covers both forms of path traversal.
    let Some(enclosed) = entry.enclosed_name() else {
        return Err(traversal(raw_name));
    };
    if entry.is_dir() {
        return Ok(None);
    }

    // Rebuild the path as a forward-slash string from the sanitized
    // components, so `.` segments collapse to one canonical key and
    // the ignore filter matches against exactly that key.
    //
    // `enclosed_name` yields only `Normal` components, but a `\`
    // inside one is an ordinary filename character on Unix, so the
    // segments are re-checked by
    // [`crate::extract::archive_entry_segments`], which treats it as
    // a separator the way pnpm does.
    let joined: String = enclosed
        .components()
        .map(|component| match component {
            Component::Normal(name) => name.to_string_lossy().into_owned(),
            _ => unreachable!("enclosed_name returns only Normal components: {:?}", enclosed),
        })
        .collect::<Vec<_>>()
        .join("/");
    let Some(segments) = crate::extract::archive_entry_segments(&joined) else {
        return Err(traversal(raw_name));
    };
    let normalized = segments.join("/");

    // Strip the archive's top-level basename (`prefix` on
    // `pnpm_lockfile::BinaryResolution`) so the ignore filter
    // sees paths relative to the archive root.
    let cleaned = match basename_prefix {
        Some(prefix) => normalized
            .strip_prefix(prefix)
            .unwrap_or(&normalized)
            .to_string(),
        None => normalized,
    };
    // An entry whose name was exactly the prefix directory leaves no
    // relative payload behind.
    if cleaned.is_empty() || ignore_file_pattern.is_some_and(|filter| filter(&cleaned)) {
        return Ok(None);
    }
    Ok(Some(cleaned))
}
