//! Port of pnpm's
//! [`parseBareSpecifier.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts).
//!
//! Decides whether a wanted dep is a local-filesystem shape (and which
//! protocol — `link:` vs `file:`) and builds the [`LocalPackageSpec`]
//! the resolver consumes.

use std::path::{Component, Path, PathBuf};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_resolving_resolver_base::PkgResolutionId;

/// The wanted-dependency slice the local resolver consumes. Mirrors
/// pnpm's
/// [`WantedLocalDependency`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L21-L24).
#[derive(Debug, Default, Clone)]
pub struct WantedLocalDependency {
    pub bare_specifier: String,
    /// `dependenciesMeta[*].injected` for this entry. When set on a
    /// directory dep the resolver picks `file:` (copy semantics)
    /// instead of `link:` (symlink semantics).
    pub injected: bool,
}

/// Parsed local-spec the resolver chain consumes. Mirrors pnpm's
/// [`LocalPackageSpec`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L14-L20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalPackageSpec {
    /// Where the directory will be addressed from inside the lockfile.
    /// For directories: a normalized path string (relative to the
    /// lockfile dir for injected file:, absolute for link:).
    pub dependency_path: String,
    /// Absolute path the resolver actually inspects (the location of
    /// `package.json` for directories, the tarball file for files).
    pub fetch_spec: PathBuf,
    /// `PkgResolutionId` upstream calls this — the branded identifier
    /// the install layer uses to dedupe and key into the lockfile.
    /// Formatted as `<protocol><normalized-path>`.
    pub id: PkgResolutionId,
    pub kind: LocalSpecKind,
    /// Normalized echo of the bare specifier (with the chosen
    /// protocol prefix). The dispatcher writes this back to the
    /// manifest spec when `add` / `update` runs.
    pub normalized_bare_specifier: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalSpecKind {
    Directory,
    File,
}

/// Options shared by [`parse_local_scheme`] and [`parse_local_path`].
/// Mirrors upstream's
/// [`{ preserveAbsolutePaths }`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L40-L44)
/// option bag.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ParseOptions {
    pub preserve_absolute_paths: bool,
}

/// `path:` is rejected so users get the same nudge they'd get from
/// pnpm. Mirrors upstream's
/// [`PathIsUnsupportedProtocolError`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L27-L36).
#[derive(Debug, Display, Error, Diagnostic, Clone)]
#[display(
    "Local dependencies via `path:` protocol are not supported. \
     Use the `link:` protocol for folder dependencies and `file:` for local tarballs"
)]
#[diagnostic(code(PATH_IS_UNSUPPORTED_PROTOCOL))]
pub struct PathProtocolNotSupportedError {
    pub bare_specifier: String,
    pub protocol: String,
}

/// Parse a wanted dep with an explicit local-scheme prefix
/// (`link:` / `workspace:` / `file:`). Returns `Ok(None)` when the
/// specifier doesn't carry one of those prefixes; returns
/// `Err(PathProtocolNotSupportedError)` for `path:`.
///
/// Mirrors upstream's
/// [`parseLocalScheme`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L38-L55).
pub(crate) fn parse_local_scheme(
    wd: &WantedLocalDependency,
    project_dir: &Path,
    lockfile_dir: &Path,
    opts: ParseOptions,
) -> Result<Option<LocalPackageSpec>, PathProtocolNotSupportedError> {
    let bare = wd.bare_specifier.as_str();
    if bare.starts_with("link:") || bare.starts_with("workspace:") {
        return Ok(Some(from_local(wd, project_dir, lockfile_dir, LocalSpecKind::Directory, opts)));
    }
    if bare.starts_with("file:") {
        let kind =
            if is_tarball_filename(bare) { LocalSpecKind::File } else { LocalSpecKind::Directory };
        return Ok(Some(from_local(wd, project_dir, lockfile_dir, kind, opts)));
    }
    if let Some(rest) = bare.strip_prefix("path:") {
        let _ = rest;
        return Err(PathProtocolNotSupportedError {
            bare_specifier: bare.to_string(),
            protocol: "path:".to_string(),
        });
    }
    Ok(None)
}

/// Parse a wanted dep by path shape alone — no scheme prefix. The
/// dispatcher calls this *after* [`parse_local_scheme`] so explicit
/// `link:`/`file:`/`workspace:`/`path:` prefixes don't slip through.
///
/// Mirrors upstream's
/// [`parseLocalPath`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L57-L73).
pub(crate) fn parse_local_path(
    wd: &WantedLocalDependency,
    project_dir: &Path,
    lockfile_dir: &Path,
    opts: ParseOptions,
) -> Option<LocalPackageSpec> {
    let bare = wd.bare_specifier.as_str();
    if is_tarball_filename(bare) || contains_path_sep(bare) || is_filespec(bare) {
        let kind =
            if is_tarball_filename(bare) { LocalSpecKind::File } else { LocalSpecKind::Directory };
        return Some(from_local(wd, project_dir, lockfile_dir, kind, opts));
    }
    None
}

/// Build the final [`LocalPackageSpec`] from a wanted dep that has
/// already been claimed by either entry point.
///
/// Mirrors upstream's
/// [`fromLocal`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L75-L136).
fn from_local(
    wd: &WantedLocalDependency,
    project_dir: &Path,
    lockfile_dir: &Path,
    kind: LocalSpecKind,
    opts: ParseOptions,
) -> LocalPackageSpec {
    let bare = wd.bare_specifier.as_str();
    let spec = normalize_specifier(bare);

    let protocol: &'static str = if bare.starts_with("file:") {
        "file:"
    } else if bare.starts_with("link:")
        || (matches!(kind, LocalSpecKind::Directory) && !wd.injected)
    {
        "link:"
    } else {
        "file:"
    };

    let (fetch_spec, normalized_bare_specifier) = if let Some(rest) = strip_tilde_prefix(&spec) {
        let home = home::home_dir().unwrap_or_default();
        let fetched = resolve_path(&home, rest);
        let normalized = format!("{protocol}{spec}");
        (fetched, normalized)
    } else {
        let fetched = resolve_path(project_dir, &spec);
        if is_absolute_specifier(&spec) {
            (fetched, format!("{protocol}{spec}"))
        } else {
            let relative =
                forward_slashes(pathdiff::diff_paths(&fetched, project_dir).map_or_else(
                    || fetched.display().to_string(),
                    |path| path.display().to_string(),
                ));
            let fetch_spec = fetched;
            (fetch_spec, format!("{protocol}{relative}"))
        }
    };

    // After upstream's `protocol = type === 'directory' && !injected ? 'link:' : 'file:'` step,
    // upstream re-uses the `injected` variable to mean "is the dep
    // copy-shaped" (`protocol === 'file:'`) for the dependencyPath /
    // id calculations below. Match the rebind explicitly so the next
    // few lines read identically.
    let copy_shaped = protocol == "file:";

    let dependency_path = if copy_shaped {
        normalize_relative_or_absolute(lockfile_dir, &fetch_spec, &spec, opts)
    } else {
        forward_slashes(fetch_spec.display().to_string())
    };

    let id_value = if !copy_shaped
        && (matches!(kind, LocalSpecKind::Directory) || project_dir == lockfile_dir)
    {
        format!(
            "{protocol}{}",
            normalize_relative_or_absolute(project_dir, &fetch_spec, &spec, opts),
        )
    } else {
        format!(
            "{protocol}{}",
            normalize_relative_or_absolute(lockfile_dir, &fetch_spec, &spec, opts),
        )
    };

    LocalPackageSpec {
        dependency_path,
        fetch_spec,
        id: PkgResolutionId::from(id_value),
        kind,
        normalized_bare_specifier,
    }
}

/// Mirror upstream's
/// [`bareSpecifier.replace(...)`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L82-L84)
/// chain:
///
/// 1. Replace all `\` with `/`.
/// 2. Drive-letter prefix: `^(file|link|workspace):/*([A-Z]:)` → `$1`.
/// 3. `^(file|link|workspace):(?:/*([~./]))?` → `$1`. The captured
///    char class **includes `/`**, so a leading slash after the
///    protocol survives (collapsed to a single one).
fn normalize_specifier(bare: &str) -> String {
    let forward = bare.replace('\\', "/");
    let Some(after_proto) =
        ["file:", "link:", "workspace:"].iter().find_map(|proto| forward.strip_prefix(proto))
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

/// Resolve `spec` against `where_dir`, mirroring Node's
/// [`path.resolve`](https://nodejs.org/api/path.html#pathresolvepaths)
/// behavior: an absolute `spec` is returned unchanged; otherwise the
/// host's path resolver joins the two and canonicalises the result.
fn resolve_path(where_dir: &Path, spec: &str) -> PathBuf {
    if is_absolute_specifier(spec) {
        return PathBuf::from(spec);
    }
    let mut joined = where_dir.to_path_buf();
    joined.push(spec);
    normalize_components(&joined)
}

/// Collapse `.` and `..` components the way Node's `path.resolve`
/// does (purely lexically — no syscalls). Preserves the absolute /
/// relative distinction of the input.
fn normalize_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `normalizeRelativeOrAbsolute` from upstream
/// [`fromLocal`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L109-L117).
/// When `preserveAbsolutePaths` is on and the input spec is absolute,
/// the result keeps the absolute form (slash-normalised); otherwise
/// the result is relative to `relative_to`.
fn normalize_relative_or_absolute(
    relative_to: &Path,
    from_path: &Path,
    original_spec: &str,
    opts: ParseOptions,
) -> String {
    if opts.preserve_absolute_paths && is_absolute_specifier(original_spec) {
        return forward_slashes(from_path.display().to_string());
    }
    let relative = pathdiff::diff_paths(from_path, relative_to)
        .map_or_else(|| from_path.display().to_string(), |path| path.display().to_string());
    forward_slashes(relative)
}

fn forward_slashes(input: String) -> String {
    if input.contains('\\') { input.replace('\\', "/") } else { input }
}

/// Match upstream's `isAbsolutePath` regex (`/^\/|^[A-Z]:/i`).
fn is_absolute_specifier(spec: &str) -> bool {
    let mut chars = spec.chars();
    match chars.next() {
        Some('/') => true,
        Some(c) if c.is_ascii_alphabetic() => chars.next() == Some(':'),
        _ => false,
    }
}

/// Match upstream's `isFilespec` regex:
/// - Windows: `/^(?:[./\\]|~\/|[a-z]:)/i`
/// - POSIX:   `/^(?:[./]|~\/|[a-z]:)/i`
///
/// Implemented uniformly because pacquet doesn't need the `\\`
/// alternative outside the bigger normalize step (`is_filespec` is
/// only consulted on already-forward-slashed paths in the upstream
/// flow because `parse_local_path` only inspects `bare_specifier`
/// which hasn't been normalised yet; we accept the backslash for
/// Windows-host inputs to keep parity).
fn is_filespec(spec: &str) -> bool {
    let mut chars = spec.chars();
    match chars.next() {
        Some('.' | '/' | '\\') => true,
        Some('~') => chars.next() == Some('/'),
        Some(c) if c.is_ascii_alphabetic() => chars.next() == Some(':'),
        _ => false,
    }
}

fn is_drive_letter_prefix(spec: &str) -> bool {
    let mut chars = spec.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic()) && matches!(chars.next(), Some(':'))
}

fn strip_tilde_prefix(spec: &str) -> Option<&str> {
    spec.strip_prefix("~/")
}

fn is_tarball_filename(bare: &str) -> bool {
    let lower = bare.to_ascii_lowercase();
    lower.ends_with(".tgz") || lower.ends_with(".tar.gz") || lower.ends_with(".tar")
}

fn contains_path_sep(bare: &str) -> bool {
    bare.contains(std::path::MAIN_SEPARATOR) || bare.contains('/')
}
