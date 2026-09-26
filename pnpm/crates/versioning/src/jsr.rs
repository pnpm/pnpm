use std::{
    fs, io,
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
    contents: String,
}

impl JsrManifestUpdate {
    pub fn write(&self) -> Result<(), VersioningError> {
        pnpm_fs::write_atomic(&self.path, self.contents.as_bytes())
            .map_err(|source| VersioningError::Write { path: self.path.clone(), source })
    }
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
        let mut contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(VersioningError::Read { path, source }),
        };
        let Some(span) = version_span(&path, &contents)? else {
            continue;
        };
        let literal = serde_json::to_string(new_version).expect("serialize a string");
        contents.replace_range(span, &literal);
        updates.push(JsrManifestUpdate { path, contents });
    }
    Ok(updates)
}

/// The span of the manifest's top-level `version` string literal, or `None`
/// when it has no string `version`. `jsr.jsonc` is parsed as JSON5, a
/// superset of JSONC.
fn version_span(path: &Path, text: &str) -> Result<Option<Range<usize>>, VersioningError> {
    let parsed = if path
        .extension()
        .is_some_and(|extension| extension == "jsonc")
    {
        pnpm_package_manifest::parse_json5_manifest(text).map_err(|error| error.to_string())
    } else {
        pnpm_package_manifest::parse_manifest(text).map_err(|error| error.to_string())
    };
    let invalid =
        |reason: String| VersioningError::InvalidJsrManifest { path: path.to_path_buf(), reason };
    let manifest = parsed.map_err(invalid)?;
    if !manifest.get("version").is_some_and(Value::is_string) {
        return Ok(None);
    }
    top_level_version_span(text)
        .map(Some)
        .ok_or_else(|| invalid("could not locate its version field".to_string()))
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Str(Range<usize>),
    Punct(u8),
    Other,
}

/// The byte span, quotes included, of the root object's `version` string in
/// `text`, which must already have parsed as a JSONC object.
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
        if byte == b'"' {
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
    let mut index = start + 1;
    while index < bytes.len() && bytes[index] != b'"' {
        index += if bytes[index] == b'\\' { 2 } else { 1 };
    }
    (index + 1).min(bytes.len())
}

#[cfg(test)]
mod tests;
