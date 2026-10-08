/// How npm ranks a package-root file as the package's README. A higher
/// variant wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadmeKind {
    /// Exactly `README`. npm matches the bare name case-sensitively.
    Bare,
    /// `README.*` whose extension npm's `/.m?a?r?k?d?o?w?n?$/i` accepts,
    /// other than `README.md`.
    Markdown,
    /// `README.md` in any case.
    ReadmeMd,
}

/// Classify a filename as a README candidate, or `None` if npm would not
/// use it as the package's README.
#[must_use]
pub fn readme_kind(file_name: &str) -> Option<ReadmeKind> {
    let lower = file_name.to_ascii_lowercase();
    if lower == "readme.md" {
        return Some(ReadmeKind::ReadmeMd);
    }
    if file_name == "README" {
        return Some(ReadmeKind::Bare);
    }
    let extension = lower
        .strip_prefix("readme.")?
        .rsplit('.')
        .next()
        .unwrap_or_default();
    // Suffix letters must occur in `markdown` order without repeats.
    let mut markdown = "markdown".chars();
    extension
        .chars()
        .all(|character| markdown.any(|expected| expected == character))
        .then_some(ReadmeKind::Markdown)
}

/// Whether README `candidate` should replace the `current` selection. A
/// higher [`ReadmeKind`] wins. Equal kinds keep the filename that sorts
/// lower by UTF-16 code units, the order the TypeScript CLI compares
/// strings in, so the choice does not depend on directory or archive order.
/// An equal filename replaces, so the last duplicate archive entry wins as
/// it would on extraction.
#[must_use]
pub fn is_preferred_readme(
    candidate: (ReadmeKind, &str),
    current: Option<(ReadmeKind, &str)>,
) -> bool {
    current.is_none_or(|current| {
        candidate.0
            .cmp(&current.0)
            .then_with(|| current.1.encode_utf16().cmp(candidate.1.encode_utf16()))
            != std::cmp::Ordering::Less
    })
}

/// Decode README bytes for publish metadata, replacing invalid UTF-8
/// rather than failing the publish.
#[must_use]
pub fn decode_readme(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}
