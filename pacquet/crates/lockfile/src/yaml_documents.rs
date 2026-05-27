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
const YAML_DOCUMENT_SEPARATOR: &str = "\n---\n";

/// Document-stream marker at the very start of a file. Matches
/// upstream's
/// [`YAML_DOCUMENT_START`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/yamlDocuments.ts#L7).
const YAML_DOCUMENT_START: &str = "---\n";

/// Extract the main lockfile document (second YAML document) from a
/// combined file. Mirrors upstream's
/// [`extractMainDocument`](https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/yamlDocuments.ts#L63-L68):
///
/// - If the content starts with `---\n`, returns the slice after the
///   next `\n---\n` separator. An empty slice (no second document
///   present) means the file is env-only.
/// - Otherwise the file is single-document and the input is returned
///   verbatim.
pub fn extract_main_document(content: &str) -> &str {
    let Some(rest) = content.strip_prefix(YAML_DOCUMENT_START) else {
        return content;
    };
    match rest.find(YAML_DOCUMENT_SEPARATOR) {
        Some(idx) => &rest[idx + YAML_DOCUMENT_SEPARATOR.len()..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::extract_main_document;

    #[test]
    fn returns_entire_content_when_it_does_not_start_with_separator() {
        let content = "lockfileVersion: 9.0\npackages: {}\n";
        assert_eq!(extract_main_document(content), content);
    }

    #[test]
    fn returns_empty_string_when_content_starts_with_separator_but_has_no_second_separator() {
        let content = "---\nfoo: bar\n";
        assert_eq!(extract_main_document(content), "");
    }

    #[test]
    fn returns_the_second_document_from_a_combined_file() {
        let main = "lockfileVersion: 9.0\npackages: {}\n";
        let combined = format!("---\nfoo: bar\n---\n{main}");
        assert_eq!(extract_main_document(&combined), main);
    }
}
