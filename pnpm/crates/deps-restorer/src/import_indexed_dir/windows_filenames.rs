use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

pub(super) struct SanitizedFilenames {
    pub paths: HashMap<String, PathBuf>,
    pub renamed: Vec<String>,
}

/// Rename invalid Windows filename components, as pnpm 11's fallback does.
pub(super) fn sanitize_filenames(
    cas_paths: &HashMap<String, PathBuf>,
) -> Option<SanitizedFilenames> {
    let mut paths = HashMap::with_capacity(cas_paths.len());
    let mut renamed = Vec::new();
    let mut entries: Vec<_> = cas_paths.iter().collect();
    entries.sort_unstable_by_key(|(filename, _)| *filename);
    for (filename, source) in entries {
        let sanitized = sanitize_path(filename);
        if sanitized.is_empty() {
            return None;
        }
        if sanitized != *filename {
            renamed.push(filename.clone());
        }
        paths.insert(sanitized, source.clone());
    }
    if renamed.is_empty() || has_file_dir_conflict(&paths) {
        return None;
    }
    Some(SanitizedFilenames { paths, renamed })
}

/// Whether one path is a proper ancestor of another, compared
/// case-insensitively as Windows does: `foo?` and `foo/bar` would need
/// `foo` to be both a file and a directory.
fn has_file_dir_conflict(paths: &HashMap<String, PathBuf>) -> bool {
    let lowercase_paths: HashSet<String> = paths
        .keys()
        .map(|path| path.to_lowercase())
        .collect();
    lowercase_paths
        .iter()
        .any(|path| {
            path.match_indices('/')
                .any(|(slash, _)| lowercase_paths.contains(&path[..slash]))
        })
}

fn is_stripped_char(ch: char) -> bool {
    matches!(ch, '\0'..='\x1f' | '\u{80}'..='\u{9f}' | '?' | '<' | '>' | ':' | '"' | '\\' | '*' | '|')
}

fn sanitize_path(filename: &str) -> String {
    // Node's path.join ignores empty components; a leading slash would reset PathBuf::join.
    filename
        .split('/')
        .map(sanitize_component)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn sanitize_component(part: &str) -> String {
    let mut result: String = part
        .chars()
        .filter(|&ch| !is_stripped_char(ch))
        .collect();
    if is_windows_device_name(&result) {
        result.clear();
    }
    result.truncate(
        result
            .trim_end_matches(['.', ' '])
            .len(),
    );
    if result.len() > 255 {
        let mut end = 255;
        while !result.is_char_boundary(end) {
            end -= 1;
        }
        result.truncate(end);
    }
    result
}

fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && bytes[3].is_ascii_digit()
}

#[cfg(test)]
mod tests;
