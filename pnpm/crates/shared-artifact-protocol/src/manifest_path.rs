use super::ArtifactProtocolError;

pub fn validate_manifest_path(path: &str) -> Result<(), ArtifactProtocolError> {
    if path.is_empty() || path.len() > 4_096 {
        return Err(invalid_path(
            path,
            "path length is outside the allowed range",
        ));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(invalid_path(path, "absolute paths are not allowed"));
    }
    if path.as_bytes().get(1) == Some(&b':') {
        return Err(invalid_path(path, "Windows drive paths are not allowed"));
    }
    if path.contains('\\') {
        return Err(invalid_path(path, "backslash separators are not allowed"));
    }
    if path.chars().any(char::is_control) {
        return Err(invalid_path(path, "control characters are not allowed"));
    }
    if path
        .split('/')
        .any(|segment| {
            segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.contains(':')
                || is_windows_reserved_name(segment)
                || segment.ends_with('.')
                || segment.ends_with(' ')
        })
    {
        return Err(invalid_path(
            path,
            "empty, dot, parent, and Windows-normalized segments are not allowed",
        ));
    }
    Ok(())
}

fn is_windows_reserved_name(segment: &str) -> bool {
    let basename = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .to_ascii_lowercase();
    matches!(basename.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"]
            .iter()
            .any(|prefix| {
                basename
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| {
                        matches!(
                            suffix,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³",
                        )
                    })
            })
}

pub(super) fn invalid_path(path: &str, reason: &str) -> ArtifactProtocolError {
    ArtifactProtocolError::InvalidManifest(format!("unsafe path {path:?}: {reason}"))
}
