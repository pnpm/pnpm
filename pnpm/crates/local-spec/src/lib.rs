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
        let protocol = if specifier.starts_with("file:") {
            LocalSpecProtocol::File
        } else if specifier.starts_with("link:") {
            LocalSpecProtocol::Link
        } else {
            return None;
        };
        Some(Self::anchor(Some(protocol), &normalize_specifier(specifier), base_dir))
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
        bare_path_is_unambiguous(specifier)
            .then(|| Self::anchor(None, &normalize_specifier(specifier), base_dir))
    }

    /// `pkg_path` has already been through [`normalize_specifier`], so
    /// it carries no protocol and reads the way the resolver reads it.
    fn anchor(protocol: Option<LocalSpecProtocol>, pkg_path: &str, base_dir: &Path) -> Self {
        let candidate = Path::new(pkg_path);
        // Absoluteness is decided by shape rather than by `Path`, which
        // disagrees on Windows about a rooted path carrying no drive
        // prefix and would re-anchor one the resolver resolves from the
        // filesystem root.
        let specified_via_relative_path =
            !names_its_own_location(pkg_path) && !pkg_path.starts_with("~/");
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
        Some('~') => matches!(chars.next(), Some('/' | '\\')),
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
///
/// A drive-prefixed path cannot be made unambiguous by a prefix, since
/// `./C:/x` names something else entirely. A tarball takes `file:`
/// instead, which is what the local resolver resolves it under either
/// way, so naming the protocol cannot change how it materializes.
///
/// A directory stays bare. Its protocol is not ours to choose: the
/// resolver reads a protocol-less directory as `link:` only while the
/// dependency is not injected, and as `file:` when it is. An explicit
/// `link:` would outrank that and silently reference an injected
/// package in place instead of copying it, so the drive-prefixed
/// spelling keeps its ambiguity rather than trade it for a wrong
/// materialization.
fn without_protocol(path: String) -> String {
    if is_drive_letter_prefix(&path) {
        return if is_tarball_filename(&path) {
            format!("{}{path}", LocalSpecProtocol::File.as_str())
        } else {
            path
        };
    }
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
///
/// Declining the shape on the way in does not settle it, because a
/// protocol-less path re-anchored onto a Windows drive puts the
/// ambiguity back. [`without_protocol`] resolves that only where it is
/// free to: a tarball takes `file:`, and a directory keeps the
/// ambiguity rather than trade it for a wrong materialization.
fn bare_path_is_unambiguous(specifier: &str) -> bool {
    !is_drive_letter_prefix(specifier) && is_filespec(specifier)
}

/// Whether the spec opens with `<letter>:`, which reads as a Windows
/// drive path and as a single-letter named-registry alias alike.
fn is_drive_letter_prefix(spec: &str) -> bool {
    let mut chars = spec.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic()) && chars.next() == Some(':')
}

/// Whether the path names a location on its own rather than one
/// measured from somewhere else, decided the way the local resolver
/// decides it: a leading `/`, or a `<letter>:` drive prefix.
///
/// [`Path::is_absolute`] cannot stand in. On Windows it reads a rooted
/// path such as `/foo` as relative, because it carries no prefix, so
/// the two would disagree about the same catalog entry depending on
/// which host resolved it.
fn names_its_own_location(path: &str) -> bool {
    let mut chars = path.chars();
    match chars.next() {
        Some('/') => true,
        Some(first) if first.is_ascii_alphabetic() => chars.next() == Some(':'),
        _ => false,
    }
}

/// Normalize a specifier the way the local resolver reads one, before
/// any of its path handling:
///
/// 1. Every `\` becomes `/`.
/// 2. A `file:` / `link:` / `workspace:` protocol is dropped, with the
///    slashes that follow it.
/// 3. A drive prefix left by step 2 stands on its own, in either case,
///    so `file:///C:/pkg` reads as `C:/pkg`.
/// 4. Otherwise, when slashes did follow the protocol, one is restored
///    — unless what remains opens with `~` or `.`, which keeps the path
///    relative: `file:/./deps/x` is the project's `./deps/x`, not the
///    root's `/deps/x`.
///
/// A specifier carrying no such protocol gets step 1 alone.
#[must_use]
pub fn normalize_specifier(bare: &str) -> String {
    let forward = bare.replace('\\', "/");
    let Some(after_proto) = ["file:", "link:", "workspace:"]
        .iter()
        .find_map(|proto| forward.strip_prefix(proto))
    else {
        return forward;
    };
    let after_slashes = after_proto.trim_start_matches('/');
    if is_drive_letter_prefix(after_slashes) {
        return after_slashes.to_string();
    }
    match after_proto.chars().next() {
        Some('/') => {
            let trimmed = after_slashes;
            if let Some(c) = trimmed.chars().next()
                && matches!(c, '~' | '.')
            {
                trimmed.to_string()
            } else {
                let mut result = String::with_capacity(trimmed.len() + 1);
                result.push('/');
                result.push_str(trimmed);
                result
            }
        }
        _ => after_proto.to_string(),
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
