use super::ImportIndexedDirError;
use std::{collections::HashMap, path::PathBuf};

pub(super) struct SanitizedFilenames {
    pub paths: HashMap<String, PathBuf>,
    pub renamed: Vec<String>,
}

/// Rename invalid Windows filename components, as pnpm 11's fallback does.
pub(super) fn sanitize_filenames(
    cas_paths: &HashMap<String, PathBuf>,
) -> Result<Option<SanitizedFilenames>, ImportIndexedDirError> {
    if !cas_paths
        .keys()
        .any(|filename| filename.split('/').any(needs_sanitizing))
    {
        return Ok(None);
    }
    let mut paths = HashMap::with_capacity(cas_paths.len());
    let mut renamed = Vec::new();
    let mut originals = Vec::with_capacity(cas_paths.len());
    for (filename, source) in cas_paths {
        let sanitized = sanitize_path(filename)?;
        if sanitized != *filename {
            renamed.push(filename.clone());
        }
        paths.insert(sanitized.clone(), source.clone());
        originals.push((filename, sanitized));
    }
    check_collisions(&originals)?;
    check_short_name_aliases(&originals)?;
    renamed.sort();
    Ok(Some(SanitizedFilenames { paths, renamed }))
}

fn needs_sanitizing(part: &str) -> bool {
    part.ends_with(['.', ' '])
        || is_windows_device_name(part, false)
        || part.encode_utf16().count() > 255
        || part
            .bytes()
            .any(|byte| {
                matches!(byte, 0..=31 | b'?' | b'<' | b'>' | b':' | b'"' | b'\\' | b'*' | b'|')
            })
}

fn sanitize_path(filename: &str) -> Result<String, ImportIndexedDirError> {
    let mut parts = Vec::new();
    for part in filename.split('/') {
        let sanitized = sanitize_component(part);
        if sanitized.is_empty() {
            return Err(ImportIndexedDirError::InvalidFilename {
                filename: filename.to_string(),
                reason: "a path component would become empty",
            });
        }
        parts.push(sanitized);
    }
    Ok(parts.join("/"))
}

fn sanitize_component(part: &str) -> String {
    let mut result: String = part
        .chars()
        .filter(|&ch| {
            !matches!(ch, '\0'..='\x1f' | '\u{80}'..='\u{9f}' | '?' | '<' | '>' | ':' | '"' | '\\' | '*' | '|')
        })
        .collect();
    if is_windows_device_name(&result, true) {
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

fn is_windows_device_name(name: &str, include_zero: bool) -> bool {
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
        && ((b'1'..=b'9').contains(&bytes[3]) || include_zero && bytes[3] == b'0')
}

fn check_short_name_aliases(originals: &[(&String, String)]) -> Result<(), ImportIndexedDirError> {
    let siblings = collect_sibling_names(originals);
    for (original, sanitized) in originals {
        let mut parent = String::new();
        for component in sanitized.split('/') {
            if siblings[&parent.to_uppercase()]
                .iter()
                .any(|other| other != component && may_be_short_alias(component, other))
            {
                return Err(ImportIndexedDirError::InvalidFilename {
                    filename: (*original).clone(),
                    reason: "the path may collide with a Windows short filename",
                });
            }
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(component);
        }
    }
    Ok(())
}

fn collect_sibling_names(originals: &[(&String, String)]) -> HashMap<String, Vec<String>> {
    let mut siblings: HashMap<String, Vec<String>> = HashMap::new();
    for (_, sanitized) in originals {
        let mut parent = String::new();
        for component in sanitized.split('/') {
            let names = siblings.entry(parent.to_uppercase()).or_default();
            if !names
                .iter()
                .any(|name| name == component)
            {
                names.push(component.to_string());
            }
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(component);
        }
    }
    siblings
}

fn may_be_short_alias(component: &str, other: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component);
    let Some((prefix, suffix)) = stem.rsplit_once('~') else {
        return false;
    };
    if !component.is_ascii()
        || stem.len() > 8
        || component
            .split_once('.')
            .is_some_and(|(_, ext)| ext.len() > 3 || ext.contains('.'))
        || prefix.is_empty()
        || suffix.is_empty()
        || !suffix.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        return false;
    }
    let other_stem = other
        .split('.')
        .next()
        .unwrap_or(other)
        .replace(' ', "");
    let ascii_prefix: String = other_stem
        .chars()
        .take_while(char::is_ascii)
        .collect();
    let ascii_prefix = ascii_prefix.to_ascii_uppercase();
    let prefix = prefix.to_ascii_uppercase();
    ascii_prefix.starts_with(&prefix)
        || (ascii_prefix.len() < other_stem.len() && prefix.starts_with(&ascii_prefix))
}

fn check_collisions(originals: &[(&String, String)]) -> Result<(), ImportIndexedDirError> {
    let mut prefixes: HashMap<String, String> = HashMap::new();
    for (original, sanitized) in originals {
        let mut original_prefix = String::new();
        let mut sanitized_prefix = String::new();
        for (part, sanitized_part) in original
            .split('/')
            .zip(sanitized.split('/'))
        {
            if !original_prefix.is_empty() {
                original_prefix.push('/');
                sanitized_prefix.push('/');
            }
            original_prefix.push_str(part);
            sanitized_prefix.push_str(sanitized_part);
            let key = sanitized_prefix.to_uppercase();
            if prefixes
                .insert(key, original_prefix.clone())
                .is_some_and(|previous| previous != original_prefix)
            {
                return Err(ImportIndexedDirError::InvalidFilename {
                    filename: (*original).clone(),
                    reason: "the renamed path would collide with another package entry",
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
