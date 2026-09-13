use super::TarballError;
/// Validate and clean one archive entry path: reject traversal, drop
/// the top-level package directory (`package/`), and join the remaining
/// segments with forward slashes.
///
/// Rejected rather than normalized so a tampered tarball is visible
/// instead of silently landing outside the store.
///
/// An entry that is only one segment long keeps that segment. Such an
/// entry sits at the archive root — beside `package/`, or in a flat
/// archive with no wrapping directory at all — so there is no
/// top-level directory on it to drop, and pnpm keys it by its own name
/// (`parseString` in `parseTarball.ts` advances past the first
/// separator, which a single segment has none of). Dropping the segment
/// instead would leave nothing to key the file by, and rejecting the
/// entry would fail an archive that every other installer accepts. A
/// lone `.` is the exception: it names the archive root rather than
/// anything inside it, so there is no file for a key to address.
///
/// Joined by hand rather than with `PathBuf`, whose native separator
/// would desynchronize these keys from pnpm's always-forward-slashed
/// path layer and the `index.db` both implementations share. Callers
/// pass the `to_string_lossy` rendering, which coerces non-UTF-8 bytes
/// to U+FFFD per component.
pub(crate) fn clean_archive_entry_path(raw: &str) -> Result<String, TarballError> {
    let Some(mut parts) = archive_entry_segments(raw) else {
        return Err(TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "tar entry path rejected (non-normal component, possible directory traversal): {raw:?}",
            ),
        )));
    };
    if parts.as_slice() == ["."] {
        return Err(TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("tar entry path names the archive root itself, not a file in it: {raw:?}"),
        )));
    }
    if parts.len() > 1 {
        parts.remove(0);
    }
    Ok(parts.join("/"))
}

/// Split a published archive entry's path into its segments, rejecting
/// anything that escapes the archive root.
///
/// `\` is treated as a separator, as pnpm does before it validates
/// (`parseTarball.ts`). Without that, a Windows-built entry keeps its
/// backslashes verbatim on Unix — where they are ordinary filename
/// characters — and the resulting key travels through the `index.db`
/// both implementations share to a reader that *does* treat them as
/// separators.
///
/// A leading `.` is preserved because npm's `tar` counts it as the
/// component removed by `strip: 1`. Other `.` components are ignored.
///
/// `None` for an absolute path or one climbing past the root.
pub(crate) fn archive_entry_segments(raw: &str) -> Option<Vec<&str>> {
    if raw.starts_with(['/', '\\']) {
        return None;
    }
    let mut segments = Vec::new();
    for (index, segment) in raw
        .split(['/', '\\'])
        .enumerate()
    {
        match segment {
            "" => {}
            "." if index == 0 => segments.push(segment),
            "." => {}
            ".." => return None,
            other => segments.push(other),
        }
    }
    (!segments.is_empty()).then_some(segments)
}
