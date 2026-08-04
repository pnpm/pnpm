//! Multi-document YAML helpers for `pnpm-lock.yaml`.
//!
//! Pnpm v11 writes the lockfile as a stream of up to two YAML
//! documents: an optional first document records the package-manager
//! bootstrap (the deps pulled in by `packageManager` / `devEngines`),
//! and the second document is the regular project lockfile. Pacquet
//! only consumes the second document, so this module strips the
//! leading env document before handing the content to serde.
//!
//! Every entry point here takes the *whole* file and normalizes it
//! first: a lockfile pacquet did not write may carry a UTF-8 BOM or
//! CRLF line endings (a `core.autocrlf` checkout on Windows), and the
//! document markers below would then match nothing.

use std::borrow::Cow;

/// Document-stream marker that ends one YAML document and starts the
/// next.
pub(crate) const YAML_DOCUMENT_SEPARATOR: &str = "\n---\n";

/// Document-stream marker at the very start of a file.
pub(crate) const YAML_DOCUMENT_START: &str = "---\n";

/// Strip a leading UTF-8 BOM and rewrite CRLF as LF, so the rest of the
/// lockfile machinery only ever sees the byte shape pacquet writes.
#[must_use]
pub fn normalize_lockfile_content(content: &str) -> Cow<'_, str> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    if content.contains("\r\n") {
        Cow::Owned(content.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(content)
    }
}

/// Extract the main lockfile document (second YAML document) from a
/// combined file.
#[must_use]
pub fn extract_main_document(content: &str) -> Cow<'_, str> {
    match normalize_lockfile_content(content) {
        Cow::Borrowed(content) => Cow::Borrowed(main_document_of(content)),
        Cow::Owned(content) => Cow::Owned(main_document_of(&content).to_string()),
    }
}

/// Extract the env lockfile document (first YAML document) from a
/// combined file:
///
/// - The file must begin with `---\n`; otherwise it carries no env
///   document and this returns `None`.
/// - Returns the slice between the leading `---\n` and the next
///   `\n---\n` separator. A leading `---\n` with no following separator
///   (an env-only file with no main document) also yields `None`.
#[must_use]
pub fn extract_env_document(content: &str) -> Option<Cow<'_, str>> {
    match normalize_lockfile_content(content) {
        Cow::Borrowed(content) => env_document_of(content).map(Cow::Borrowed),
        Cow::Owned(content) => env_document_of(&content).map(|doc| Cow::Owned(doc.to_string())),
    }
}

fn main_document_of(content: &str) -> &str {
    let Some(rest) = content.strip_prefix(YAML_DOCUMENT_START) else {
        return content;
    };
    match rest.find(YAML_DOCUMENT_SEPARATOR) {
        Some(idx) => &rest[idx + YAML_DOCUMENT_SEPARATOR.len()..],
        None => "",
    }
}

fn env_document_of(content: &str) -> Option<&str> {
    let rest = content.strip_prefix(YAML_DOCUMENT_START)?;
    rest.find(YAML_DOCUMENT_SEPARATOR).map(|idx| &rest[..idx])
}

#[cfg(test)]
mod tests;
