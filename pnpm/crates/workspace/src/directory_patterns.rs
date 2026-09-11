use crate::FindWorkspaceProjectsError;
use pnpm_fs::lexical_normalize_posix;
use wax::Glob;

pub(crate) fn normalize_directory_pattern(pattern: &str) -> Option<String> {
    let mut normalized = lexical_normalize_posix(pattern);
    normalized.truncate(normalized.trim_end_matches('/').len());
    if normalized.is_empty() || normalized == "." {
        return None;
    }
    Some(normalized)
}

pub(crate) fn negated_directory_pattern(
    pattern: &str,
) -> Result<Option<String>, FindWorkspaceProjectsError> {
    let Some(body) = pattern.strip_prefix('!').filter(|body| !body.starts_with('/')) else {
        return Ok(None);
    };
    let Some(directory) = normalize_directory_pattern(body) else {
        return Ok(None);
    };
    Glob::new(&directory).map_err(|error| FindWorkspaceProjectsError::InvalidGlob {
        pattern: pattern.to_string(),
        message: error.to_string(),
    })?;
    Ok(Some(directory))
}
