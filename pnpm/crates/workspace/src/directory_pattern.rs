use pnpm_fs::lexical_normalize_posix;

pub(crate) fn normalize_directory_pattern(pattern: &str) -> Option<String> {
    let mut normalized = lexical_normalize_posix(pattern);
    normalized.truncate(normalized.trim_end_matches('/').len());
    if normalized.is_empty() || normalized == "." {
        return None;
    }
    Some(normalized)
}
