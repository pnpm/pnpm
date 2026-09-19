//! A specifier naming a local path means "this path, relative to the file
//! it is written in". pnpm reads such specifiers from files that do not sit
//! in the project consuming them — `pnpm-workspace.yaml` holds the catalogs
//! and the `overrides` map — so the path has to be re-anchored before the
//! resolver, which reads every specifier relative to the importing project,
//! can see it.
//!
//! [`LocalSpec::parse`] anchors the written path at the directory of the
//! file that declared it; [`LocalSpec::render`] writes it back out for the
//! directory that consumes it. [`LocalSpec::parse`] claims the `file:` /
//! `link:` protocols, and [`LocalSpec::parse_filesystem`] claims every
//! shape that can only be a local path.
//!
//! The shape tests themselves — [`is_local_filesystem_specifier`] and the
//! two predicates behind it — live here rather than in the local resolver
//! so a caller can ask what a specifier is without depending on the code
//! that resolves it.

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

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

/// A local specifier with its path resolved against the directory it
/// was written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSpec {
    /// `None` for a specifier written as a bare path, which carries no
    /// protocol and is rendered back without one.
    protocol: Option<LocalSpecProtocol>,
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
    ///
    /// Only a path written relative to `base_dir` moves. An absolute one,
    /// and a `~/` one that the resolver expands against the home
    /// directory, name the same place from every directory.
    #[must_use]
    pub fn parse(specifier: &str, base_dir: &Path) -> Option<Self> {
        let (protocol, pkg_path) = if let Some(rest) = specifier.strip_prefix("file:") {
            (LocalSpecProtocol::File, rest)
        } else {
            (LocalSpecProtocol::Link, specifier.strip_prefix("link:")?)
        };
        Some(Self::anchor(Some(protocol), pkg_path, base_dir))
    }

    /// Parse a specifier that names a local path with or without a
    /// protocol, so a bare `./x.tgz` moves with its declaring file the
    /// way `file:./x.tgz` does.
    ///
    /// Without a protocol, only a path-prefixed specifier is claimed —
    /// `./`, `../`, `/`, `~/`. Every other shape reaches a resolver
    /// before the local one, and re-anchoring rewrites a specifier into
    /// a path-shaped one, which would move it onto the local resolver:
    /// a slash-free `repo.tgz` is a dist-tag to the npm resolver,
    /// `user/repo.tgz` is a hosted-git shorthand, and `<letter>:` is a
    /// single-letter named registry as much as a Windows drive path.
    #[must_use]
    pub fn parse_filesystem(specifier: &str, base_dir: &Path) -> Option<Self> {
        if let Some(parsed) = Self::parse(specifier, base_dir) {
            return Some(parsed);
        }
        bare_path_is_unambiguous(specifier).then(|| Self::anchor(None, specifier, base_dir))
    }

    fn anchor(protocol: Option<LocalSpecProtocol>, pkg_path: &str, base_dir: &Path) -> Self {
        // The resolver forward-slashes a specifier before it reads the
        // path, so `\foo` is absolute to it on every host, while `Path`
        // takes the backslash for an ordinary character off Windows.
        // Reading the raw string here would re-anchor a path the
        // resolver resolves from the filesystem root.
        let pkg_path = forward_slashes(pkg_path);
        let candidate = Path::new(pkg_path.as_ref());
        let specified_via_relative_path = !candidate.is_absolute() && !pkg_path.starts_with("~/");
        let absolute_path = lexical_normalize(&if specified_via_relative_path {
            base_dir.join(candidate)
        } else {
            candidate.to_path_buf()
        });
        LocalSpec { protocol, absolute_path, specified_via_relative_path }
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
        let Some(protocol) = self.protocol else {
            return without_protocol(path);
        };
        format!("{}{path}", protocol.as_str())
    }
}

/// Whether a bare specifier's shape can only mean a local file or
/// directory: the `link:` / `file:` protocols, a path-prefixed spec
/// (`./`, `../`, `~/`, absolute POSIX paths, and Windows drive paths —
/// including drive-relative ones like `C:dir`), or a bare tarball file
/// name.
///
/// Narrower than what the local resolver's own path parsing claims,
/// which also takes any spec containing a path separator. That shape is
/// statically indistinguishable from a hosted-git shorthand
/// (`user/repo`) or a named-registry alias (`gh:@scope/pkg`), and the
/// resolver chain only gets away with claiming it by running the local
/// resolver last. Callers that dispatch on specifier shape without that
/// ordering ask this instead.
#[must_use]
pub fn is_local_filesystem_specifier(bare: &str) -> bool {
    if bare.starts_with("link:") || bare.starts_with("file:") {
        return true;
    }
    if is_filespec(bare) {
        return true;
    }
    // Any other protocol — a `git+ssh:` / `https:` URL, an `npm:` alias, a
    // named-registry prefix — belongs to its own resolver, tarball-shaped
    // path or not.
    if bare.contains(':') {
        return false;
    }
    // A `#` here marks a hosted-git shorthand's committish
    // (`user/repo#release.tgz`), not a local tarball: the protocol and
    // path-prefixed forms already returned above.
    if bare.contains('#') {
        return false;
    }
    is_tarball_filename(bare)
}

/// `true` for a path-shaped spec:
/// - Windows: `/^(?:[./\\]|~\/|[a-z]:)/i`
/// - POSIX:   `/^(?:[./]|~\/|[a-z]:)/i`
///
/// Implemented uniformly (accepting the backslash on every platform):
/// the local resolver inspects a bare specifier before the normalize
/// step that forward-slashes paths, so Windows-host inputs may still
/// carry a leading `\`.
#[must_use]
pub fn is_filespec(spec: &str) -> bool {
    let mut chars = spec.chars();
    match chars.next() {
        Some('.' | '/' | '\\') => true,
        Some('~') => chars.next() == Some('/'),
        Some(c) if c.is_ascii_alphabetic() => chars.next() == Some(':'),
        _ => false,
    }
}

/// Whether a local specifier names a package tarball rather than a
/// directory. A `file:` specifier resolves to one or the other, and only
/// the directory form becomes a `link:` entry in the lockfile.
#[must_use]
pub fn is_tarball_filename(bare: &str) -> bool {
    let lower = bare.to_ascii_lowercase();
    lower.ends_with(".tgz") || lower.ends_with(".tar.gz") || lower.ends_with(".tar")
}

/// Render a path that carries no protocol, keeping it unambiguously
/// local. Re-anchoring can drop a leading `./` — `./libs/x` measured
/// from the workspace root renders as `libs/x` — and a bare
/// `<segment>/<segment>` reads as a hosted-git shorthand instead, so
/// the prefix goes back on.
fn without_protocol(path: String) -> String {
    if is_filespec(&path) { path } else { format!("./{path}") }
}

/// Whether a specifier carrying no `file:` / `link:` prefix can only be
/// a local path, so re-anchoring it cannot change which resolver claims
/// it.
///
/// Stricter than [`is_local_filesystem_specifier`], which answers a
/// different question: what a specifier *could* name, rather than which
/// resolver reaches it first. The chain runs the local path resolver
/// last, so every shape that is merely path-*like* has already been
/// claimed by then — a slash-free `repo.tgz` resolves as a dist-tag,
/// and `user/repo.tgz` as a hosted-git shorthand. Only a path-prefixed
/// specifier lands on the local resolver, and only those move.
///
/// A `<letter>:` prefix is declined with them: a single-letter named
/// registry is well-formed, so `c:pkg@1` is a registry specifier as
/// much as a drive path. Nothing is lost by that — a drive path names
/// the same place from every directory, and a drive-relative one is
/// measured from process state no caller here can see.
fn bare_path_is_unambiguous(specifier: &str) -> bool {
    !is_drive_letter_prefix(specifier) && is_filespec(specifier)
}

/// Whether the spec opens with `<letter>:`, which reads as a Windows
/// drive path and as a single-letter named-registry alias alike.
fn is_drive_letter_prefix(spec: &str) -> bool {
    let mut chars = spec.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic()) && chars.next() == Some(':')
}

/// Rewrite `\` to `/`, as the local resolver's own specifier
/// normalization does before it reads a path. A `~/` path survives it
/// unchanged, which is what leaves such a path anchored at neither the
/// declaring nor the consuming directory: the resolver expands it
/// against the home directory and records it verbatim.
fn forward_slashes(path: &str) -> Cow<'_, str> {
    if path.contains('\\') { Cow::Owned(path.replace('\\', "/")) } else { Cow::Borrowed(path) }
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
