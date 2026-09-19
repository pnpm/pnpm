//! A `file:` / `link:` specifier means "this path, relative to the file it
//! is written in". pnpm reads such specifiers from files that do not sit in
//! the project consuming them — `pnpm-workspace.yaml` holds the catalogs and
//! the `overrides` map — so the path has to be re-anchored before the
//! resolver, which reads every specifier relative to the importing project,
//! can see it.
//!
//! [`LocalSpec::parse`] anchors the written path at the directory of the
//! file that declared it; [`LocalSpec::render`] writes it back out for the
//! directory that consumes it.

use std::path::{Path, PathBuf};

use pnpm_fs::{lexical_normalize, relative_path};

/// The two local-filesystem protocols a specifier can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalSpecProtocol {
    Link,
    File,
}

impl LocalSpecProtocol {
    fn as_str(self) -> &'static str {
        match self {
            LocalSpecProtocol::Link => "link:",
            LocalSpecProtocol::File => "file:",
        }
    }
}

/// A `file:` / `link:` specifier with its path resolved against the
/// directory it was written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSpec {
    protocol: LocalSpecProtocol,
    absolute_path: PathBuf,
    /// Whether the specifier was written as a relative path. An absolute
    /// one names the same location from everywhere, so it is rendered
    /// verbatim rather than re-anchored.
    specified_via_relative_path: bool,
}

impl LocalSpec {
    /// Parse a `link:` / `file:` specifier whose relative path is written
    /// from `base_dir`. Returns `None` for any other shape — semver ranges,
    /// tarball URLs, npm-alias specs, `catalog:` and `workspace:`
    /// specifiers, and bare paths carrying no protocol.
    #[must_use]
    pub fn parse(specifier: &str, base_dir: &Path) -> Option<Self> {
        let (protocol, pkg_path) = if let Some(rest) = specifier.strip_prefix("file:") {
            (LocalSpecProtocol::File, rest)
        } else {
            (LocalSpecProtocol::Link, specifier.strip_prefix("link:")?)
        };

        let candidate = Path::new(pkg_path);
        let specified_via_relative_path = !candidate.is_absolute();
        let absolute_path = lexical_normalize(&if specified_via_relative_path {
            base_dir.join(candidate)
        } else {
            candidate.to_path_buf()
        });
        Some(LocalSpec { protocol, absolute_path, specified_via_relative_path })
    }

    /// Render the specifier for the directory that consumes it.
    /// Relative-form specifiers are re-anchored against `consumer_dir` so
    /// they read sensibly from the consumer's perspective; absolute-form
    /// ones, and every specifier when `consumer_dir` is `None`, are emitted
    /// as the absolute path.
    #[must_use]
    pub fn render(&self, consumer_dir: Option<&Path>) -> String {
        // Every branch routes through `normalize_path` so the absolute
        // shape also gets backslash → forward-slash rewriting on Windows;
        // `link:` / `file:` specifiers must use forward slashes regardless
        // of host OS.
        let path = match (self.specified_via_relative_path, consumer_dir) {
            (true, Some(dir)) => normalize_path(&relative_path(dir, &self.absolute_path)),
            _ => normalize_path(&self.absolute_path),
        };
        // A specifier that names the consumer's own directory diffs to the
        // empty string, which reads as a missing path rather than as "here".
        let path = if path.is_empty() { ".".to_string() } else { path };
        format!("{}{path}", self.protocol.as_str())
    }
}

/// Replace `\` with `/` to normalize the path. `link:` / `file:`
/// specifiers must use forward slashes regardless of host OS — the
/// lockfile and pacquet's downstream consumers expect that shape.
fn normalize_path(path: &Path) -> String {
    let display = path.display().to_string();
    if cfg!(windows) { display.replace('\\', "/") } else { display }
}

#[cfg(test)]
mod tests;
