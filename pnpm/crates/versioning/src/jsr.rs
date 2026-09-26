use std::{
    fmt, fs, io,
    ops::Range,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::error::VersioningError;

/// The JSR manifests a package may keep next to its `package.json`.
const JSR_MANIFEST_BASENAMES: [&str; 2] = ["jsr.json", "jsr.jsonc"];

/// A JSR manifest with its top-level `version` rewritten, not yet written.
#[derive(Debug)]
pub struct JsrManifestUpdate {
    pub path: PathBuf,
    original: String,
    contents: String,
}

/// The JSR manifests in `pkg_dir` that declare a top-level `version`, each with
/// only that literal set to `new_version`, so formatting and comments survive.
pub fn jsr_manifest_updates(
    pkg_dir: &Path,
    new_version: &str,
) -> Result<Vec<JsrManifestUpdate>, VersioningError> {
    let mut updates = Vec::new();
    for basename in JSR_MANIFEST_BASENAMES {
        let path = pkg_dir.join(basename);
        let original = match fs::read_to_string(&path) {
            Ok(original) => original,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(VersioningError::Read { path, source }),
        };
        if let Some(contents) = rewrite_version(&path, &original, new_version)? {
            updates.push(JsrManifestUpdate { path, original, contents });
        }
    }
    Ok(updates)
}

/// Write `updates`, then run `save_package_manifest`. If either fails, the JSR
/// manifests already written get their original contents back, so a failed
/// bump does not leave them at a version `package.json` lacks.
pub fn save_with_jsr_manifests<Error: From<VersioningError> + fmt::Display>(
    updates: &[JsrManifestUpdate],
    save_package_manifest: impl FnOnce() -> Result<(), Error>,
) -> Result<(), Error> {
    for (written, update) in updates.iter().enumerate() {
        if let Err(error) = write_file(&update.path, &update.contents) {
            return Err(restore_originals(&updates[..written], error.into()));
        }
    }
    save_package_manifest().map_err(|error| restore_originals(updates, error))
}

/// Restore every update's original contents and return `interrupted_by`, or,
/// if a restore fails, the error naming the manifest left at the new version.
fn restore_originals<Error: From<VersioningError> + fmt::Display>(
    updates: &[JsrManifestUpdate],
    interrupted_by: Error,
) -> Error {
    let mut failed_restore = None;
    for update in updates {
        if let Err(source) = pnpm_fs::write_atomic(&update.path, update.original.as_bytes()) {
            failed_restore.get_or_insert_with(|| (update.path.clone(), source));
        }
    }
    match failed_restore {
        None => interrupted_by,
        Some((path, source)) => VersioningError::RestoreJsrManifest {
            path,
            interrupted_by: interrupted_by.to_string(),
            source,
        }
        .into(),
    }
}

fn write_file(path: &Path, contents: &str) -> Result<(), VersioningError> {
    pnpm_fs::write_atomic(path, contents.as_bytes())
        .map_err(|source| VersioningError::Write { path: path.to_path_buf(), source })
}

/// `text` with its top-level `version` string set to `new_version`, or `None`
/// when it has no string `version`. The result must parse to the original
/// manifest with only `version` changed.
fn rewrite_version(
    path: &Path,
    text: &str,
    new_version: &str,
) -> Result<Option<String>, VersioningError> {
    let mut manifest = parse_jsr_manifest(path, text)?;
    if !manifest.get("version").is_some_and(Value::is_string) {
        return Ok(None);
    }
    let unlocatable = || VersioningError::InvalidJsrManifest {
        path: path.to_path_buf(),
        reason: "could not locate its top-level version field".to_string(),
    };
    let span = top_level_version_span(text).ok_or_else(unlocatable)?;
    let mut contents = text.to_string();
    contents.replace_range(span, &serde_json::to_string(new_version).expect("serialize a string"));
    manifest["version"] = Value::String(new_version.to_string());
    if parse_jsr_manifest(path, &contents)? != manifest {
        return Err(unlocatable());
    }
    Ok(Some(contents))
}

/// `jsr.jsonc` is parsed as JSON5, a superset of JSONC.
fn parse_jsr_manifest(path: &Path, text: &str) -> Result<Value, VersioningError> {
    let parsed = if path
        .extension()
        .is_some_and(|extension| extension == "jsonc")
    {
        pnpm_package_manifest::parse_json5_manifest(text).map_err(|error| error.to_string())
    } else {
        pnpm_package_manifest::parse_manifest(text).map_err(|error| error.to_string())
    };
    parsed.map_err(|reason| VersioningError::InvalidJsrManifest {
        path: path.to_path_buf(),
        reason,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Str(Range<usize>),
    Punct(u8),
    Other,
}

/// The byte span, quotes included, of the root object's quoted `version` key's
/// string value in `text`, which must already have parsed as an object.
fn top_level_version_span(text: &str) -> Option<Range<usize>> {
    let tokens = tokenize(text);
    let mut depth = 0usize;
    let mut expecting_key = false;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Punct(b'{' | b'[') => {
                depth += 1;
                expecting_key = depth == 1;
            }
            Token::Punct(b'}' | b']') => depth = depth.saturating_sub(1),
            Token::Punct(b',') => expecting_key = depth == 1,
            Token::Str(key) if expecting_key => {
                expecting_key = false;
                if &text[key.start + 1..key.end - 1] == "version"
                    && tokens.get(index + 1) == Some(&Token::Punct(b':'))
                    && let Some(Token::Str(value)) = tokens.get(index + 2)
                {
                    return Some(value.clone());
                }
            }
            _ => {}
        }
    }
    None
}

fn tokenize(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = comment_end(bytes, index) {
            index = end;
            continue;
        }
        let byte = bytes[index];
        if matches!(byte, b'"' | b'\'') {
            let end = string_end(bytes, index);
            tokens.push(Token::Str(index..end));
            index = end;
            continue;
        }
        if matches!(byte, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            tokens.push(Token::Punct(byte));
        } else if !byte.is_ascii_whitespace() {
            tokens.push(Token::Other);
        }
        index += 1;
    }
    tokens
}

/// The index past the `//` or `/* */` comment starting at `start`, if one does.
fn comment_end(bytes: &[u8], start: usize) -> Option<usize> {
    let rest = &bytes[start..];
    if rest.starts_with(b"//") {
        return Some(
            rest.iter()
                .position(|&byte| byte == b'\n')
                .map_or(bytes.len(), |offset| start + offset),
        );
    }
    if rest.starts_with(b"/*") {
        return Some(
            rest[2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(bytes.len(), |offset| start + 2 + offset + 2),
        );
    }
    None
}

/// The index past the string literal whose opening quote is at `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() && bytes[index] != quote {
        index += if bytes[index] == b'\\' { 2 } else { 1 };
    }
    (index + 1).min(bytes.len())
}

#[cfg(test)]
mod tests;
