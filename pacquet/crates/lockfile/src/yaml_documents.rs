//! Multi-document YAML helpers for `pnpm-lock.yaml`.
//!
//! Pnpm v11 writes the lockfile as a stream of up to two YAML
//! documents: an optional first document records the package-manager
//! bootstrap (the deps pulled in by `packageManager` / `devEngines`),
//! and the second document is the regular project lockfile. Pacquet
//! only consumes the second document, so this module mirrors
//! upstream's
//! [`extractMainDocument`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/yamlDocuments.ts#L57-L68)
//! to strip the leading env document before handing the content to
//! serde.

/// Document-stream marker that ends one YAML document and starts the
/// next. Matches upstream's
/// [`YAML_DOCUMENT_SEPARATOR`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/yamlDocuments.ts#L6).
pub(crate) const YAML_DOCUMENT_SEPARATOR: &str = "\n---\n";

/// Document-stream marker at the very start of a file. Matches
/// upstream's
/// [`YAML_DOCUMENT_START`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/yamlDocuments.ts#L7).
pub(crate) const YAML_DOCUMENT_START: &str = "---\n";

/// Extract the main lockfile document (second YAML document) from a
/// combined file. Mirrors upstream's
/// [`extractMainDocument`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/yamlDocuments.ts#L63-L68):
///
/// - If the content starts with `---\n`, returns the slice after the
///   next `\n---\n` separator. An empty slice (no second document
///   present) means the file is env-only.
/// - Otherwise the file is single-document and the input is returned
///   verbatim.
#[must_use]
pub fn extract_main_document(content: &str) -> &str {
    let Some(rest) = content.strip_prefix(YAML_DOCUMENT_START) else {
        return content;
    };
    match rest.find(YAML_DOCUMENT_SEPARATOR) {
        Some(idx) => &rest[idx + YAML_DOCUMENT_SEPARATOR.len()..],
        None => "",
    }
}

/// Extract the env lockfile document (first YAML document) from a
/// combined file. The synchronous counterpart to upstream's streaming
/// [`streamReadFirstYamlDocument`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/yamlDocuments.ts#L15-L60):
///
/// - The file must begin with `---\n`; otherwise it carries no env
///   document and this returns `None`.
/// - Returns the slice between the leading `---\n` and the next
///   `\n---\n` separator. A leading `---\n` with no following separator
///   (an env-only file with no main document) also yields `None`,
///   matching upstream's fall-through to `return null`.
///
/// pacquet reads the whole lockfile into memory rather than streaming,
/// so this skips upstream's chunked BOM/CRLF handling — the only
/// callers pass content pacquet itself wrote with LF line endings.
#[must_use]
pub fn extract_env_document(content: &str) -> Option<&str> {
    let rest = content.strip_prefix(YAML_DOCUMENT_START)?;
    rest.find(YAML_DOCUMENT_SEPARATOR).map(|idx| &rest[..idx])
}

#[cfg(test)]
mod tests;
