/// Paths already admitted from a single package archive.
#[derive(Default)]
pub struct ArchivePaths(std::collections::HashSet<String>);

impl ArchivePaths {
    /// Records a relative path once, ignoring ASCII case for collision checks.
    /// Directory entries may carry one trailing slash.
    pub fn insert<'a>(&mut self, path: &'a str, directory: bool) -> Option<&'a str> {
        let path = if directory { path.strip_suffix('/').unwrap_or(path) } else { path };
        if path.contains(['\\', ':', '\0'])
            || path
                .split('/')
                .any(|part| part.is_empty() || part.ends_with(['.', ' ']))
            || !self.0.insert(path.to_ascii_lowercase())
        {
            return None;
        }
        Some(path)
    }
}
