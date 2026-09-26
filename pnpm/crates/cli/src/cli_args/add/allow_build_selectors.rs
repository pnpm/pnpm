/// Split comma-separated `--allow-build` values into one selector per item,
/// as if the flag had been repeated.
///
/// A value that contains `:` is kept whole. Every `allowBuilds` key other
/// than a package name and version carries a protocol (`file:`, `link:`,
/// `https:`, `git+ssh:`, ...), and a comma can be part of its path or URL
/// (`pkg@file:./a,b`, `pkg@https://example.com/a,b.tgz`).
/// Items are trimmed. An empty item is kept, so the caller rejects it the
/// same way as `--allow-build=`.
pub(crate) fn split_allow_build_selectors(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| {
            if value.contains(':') {
                vec![value.clone()]
            } else {
                value
                    .split(',')
                    .map(|item| item.trim().to_string())
                    .collect()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
