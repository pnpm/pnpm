use std::{collections::HashMap, path::PathBuf};

pub(super) struct SanitizedFilenames {
    pub paths: HashMap<String, PathBuf>,
    pub renamed: Vec<String>,
}

/// Rename invalid Windows filename components, as pnpm 11's fallback does.
///
/// Paths that Windows treats as equal (case-insensitively) keep one source.
/// A name that needed no renaming wins over a renamed one, so `package?.json`
/// never replaces `package.json`. Otherwise the later name in sorted order wins.
pub(super) fn sanitize_filenames(
    cas_paths: &HashMap<String, PathBuf>,
) -> Option<SanitizedFilenames> {
    let mut entries = Vec::with_capacity(cas_paths.len());
    for (filename, source) in cas_paths {
        let sanitized = sanitize_path(filename);
        if sanitized.is_empty() {
            return None;
        }
        entries.push((sanitized, filename, source));
    }
    entries.sort_unstable_by_key(|(sanitized, filename, _)| (sanitized == *filename, *filename));
    let mut paths = HashMap::with_capacity(entries.len());
    let mut spelling_by_folded_path = HashMap::with_capacity(entries.len());
    let mut renamed = Vec::new();
    for (sanitized, filename, source) in entries {
        if sanitized != *filename {
            renamed.push(filename.clone());
        }
        if let Some(replaced) =
            spelling_by_folded_path.insert(sanitized.to_lowercase(), sanitized.clone())
        {
            paths.remove(&replaced);
        }
        paths.insert(sanitized, source.clone());
    }
    if renamed.is_empty() || has_file_dir_conflict(&spelling_by_folded_path) {
        return None;
    }
    Some(SanitizedFilenames { paths, renamed })
}

/// Whether one case-folded path is a proper ancestor of another: `foo?` and
/// `foo/bar` would need `foo` to be both a file and a directory.
fn has_file_dir_conflict(spelling_by_folded_path: &HashMap<String, String>) -> bool {
    spelling_by_folded_path
        .keys()
        .any(|path| {
            path.match_indices('/')
                .any(|(slash, _)| spelling_by_folded_path.contains_key(&path[..slash]))
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
