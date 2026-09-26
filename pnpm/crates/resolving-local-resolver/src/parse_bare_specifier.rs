//! Bare-specifier parsing for the local-filesystem resolver.
//!
//! Decides whether a wanted dep is a local-filesystem shape (and which
//! protocol — `link:` vs `file:`) and builds the [`LocalPackageSpec`]
//! the resolver consumes.

use std::path::{Path, PathBuf};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_fs::{lexical_normalize, relative_path};
use pnpm_local_spec::{is_filespec, is_tarball_filename, normalize_specifier};
use pnpm_resolving_resolver_base::PkgResolutionId;

/// The wanted-dependency slice the local resolver consumes.
#[derive(Debug, Default, Clone)]
pub struct WantedLocalDependency {
    pub bare_specifier: String,
    /// `dependenciesMeta[*].injected` for this entry. When set on a
    /// directory dep the resolver picks `file:` (copy semantics)
    /// instead of `link:` (symlink semantics).
    pub injected: bool,
}

/// Parsed local-spec the resolver chain consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalPackageSpec {
    /// Where the directory will be addressed from inside the lockfile.
    /// For directories: a normalized path string (relative to the
    /// lockfile dir for injected file:, absolute for link:).
    pub dependency_path: String,
    /// Absolute path the resolver actually inspects (the location of
    /// `package.json` for directories, the tarball file for files).
    pub fetch_spec: PathBuf,
    /// Branded identifier the install layer uses to dedupe and key
    /// into the lockfile. Formatted as `<protocol><normalized-path>`.
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
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ParseOptions {
    pub preserve_absolute_paths: bool,
    /// `inject-workspace-packages` config. Injects a `workspace:` directory
    /// dependency the same way the name/range workspace match in
    /// `resolving-npm-resolver` does. Other specifiers ignore it.
    pub inject_workspace_packages: bool,
}

/// `path:` is rejected so users get a nudge toward `link:` / `file:`.
#[derive(Debug, Display, Error, Diagnostic, Clone)]
#[display(
    "Local dependencies via `path:` protocol are not supported. \
     Use the `link:` protocol for folder dependencies and `file:` for local tarballs"
)]
#[diagnostic(code(ERR_PNPM_PATH_IS_UNSUPPORTED_PROTOCOL))]
pub struct PathProtocolNotSupportedError {
    pub bare_specifier: String,
    pub protocol: String,
}

/// Parse a wanted dep with an explicit local-scheme prefix
/// (`link:` / `workspace:` / `file:`). Returns `Ok(None)` when the
/// specifier doesn't carry one of those prefixes; returns
/// `Err(PathProtocolNotSupportedError)` for `path:`.
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
fn from_local(
    wd: &WantedLocalDependency,
    project_dir: &Path,
    lockfile_dir: &Path,
    kind: LocalSpecKind,
    opts: ParseOptions,
) -> LocalPackageSpec {
    let bare = wd.bare_specifier.as_str();
    let spec = normalize_specifier(bare);

    let injected =
        wd.injected || (bare.starts_with("workspace:") && opts.inject_workspace_packages);
    let protocol = local_protocol(bare, kind, injected);
    let (fetch_spec, normalized_bare_specifier) =
        fetched_and_normalized(&spec, project_dir, protocol);

    // Once the protocol is chosen, "copy-shaped" (`protocol == "file:"`)
    // drives the dependencyPath / id calculations below.
    let copy_shaped = protocol == "file:";

    let dependency_path = if copy_shaped {
        normalize_relative_or_absolute(lockfile_dir, &fetch_spec, &spec, opts)
    } else {
        forward_slashes(fetch_spec.display().to_string())
    };

    let id_base = if !copy_shaped
        && (matches!(kind, LocalSpecKind::Directory) || project_dir == lockfile_dir)
    {
        project_dir
    } else {
        lockfile_dir
    };
    let id_value =
        format!("{protocol}{}", normalize_relative_or_absolute(id_base, &fetch_spec, &spec, opts));

    LocalPackageSpec {
        dependency_path,
        fetch_spec,
        id: PkgResolutionId::from(id_value),
        kind,
        normalized_bare_specifier,
    }
}

/// The protocol a local specifier resolves under. A `link:` directory is
/// referenced in place; everything else is copied, which is what `file:`
/// means here.
fn local_protocol(bare: &str, kind: LocalSpecKind, injected: bool) -> &'static str {
    if bare.starts_with("file:") {
        return "file:";
    }
    if bare.starts_with("link:") || (matches!(kind, LocalSpecKind::Directory) && !injected) {
        return "link:";
    }
    "file:"
}

/// The path a local specifier fetches from, and the specifier the manifest
/// records for it. A `~` specifier resolves against the home directory and is
/// recorded verbatim; a relative one is recorded relative to the project.
fn fetched_and_normalized(spec: &str, project_dir: &Path, protocol: &str) -> (PathBuf, String) {
    if let Some(rest) = strip_tilde_prefix(spec) {
        let home = home::home_dir().unwrap_or_default();
        return (resolve_path(&home, rest), format!("{protocol}{spec}"));
    }
    let fetched = resolve_path(project_dir, spec);
    if is_absolute_specifier(spec) {
        return (fetched, format!("{protocol}{spec}"));
    }
    let relative = forward_slashes(relative_path(project_dir, &fetched).display().to_string());
    (fetched, format!("{protocol}{relative}"))
}

/// Resolve `spec` against `where_dir`, mirroring Node's
/// [`path.resolve`](https://nodejs.org/api/path.html#pathresolvepaths):
/// a relative `spec` is joined onto `where_dir` first, and either way
/// the result's `.` and `..` components are collapsed lexically,
/// without touching the filesystem.
fn resolve_path(where_dir: &Path, spec: &str) -> PathBuf {
    if is_absolute_specifier(spec) {
        return lexical_normalize(Path::new(spec));
    }
    lexical_normalize(&where_dir.join(spec))
}

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
    forward_slashes(relative_path(relative_to, from_path).display().to_string())
}

fn forward_slashes(input: String) -> String {
    if input.contains('\\') { input.replace('\\', "/") } else { input }
}

/// `true` for an absolute spec: a leading `/` or a `<letter>:` drive
/// prefix (`/^\/|^[A-Z]:/i`).
fn is_absolute_specifier(spec: &str) -> bool {
    let mut chars = spec.chars();
    match chars.next() {
        Some('/') => true,
        Some(c) if c.is_ascii_alphabetic() => chars.next() == Some(':'),
        _ => false,
    }
}

fn strip_tilde_prefix(spec: &str) -> Option<&str> {
    spec.strip_prefix("~/")
}

/// Resolve an unambiguous local tarball specifier to the regular file
/// inspected by the local resolver. Returns `None` for directories,
/// ambiguous bare specifiers, and non-local tarball URLs.
#[must_use]
pub fn local_tarball_path(bare: &str, project_dir: &Path) -> Option<PathBuf> {
    if !(bare.starts_with("file:") || is_filespec(bare)) || !is_tarball_filename(bare) {
        return None;
    }
    let wanted = WantedLocalDependency { bare_specifier: bare.to_string(), injected: false };
    let spec = if bare.starts_with("file:") {
        parse_local_scheme(&wanted, project_dir, project_dir, ParseOptions::default())
            .ok()
            .flatten()
    } else {
        parse_local_path(&wanted, project_dir, project_dir, ParseOptions::default())
    }?;
    (matches!(spec.kind, LocalSpecKind::File) && spec.fetch_spec.is_file()).then_some(
        spec.fetch_spec,
    )
}

/// Resolve a `file:` specifier to the path the local resolver reads: the
/// package directory, or the tarball file. Returns `None` for any other
/// specifier.
#[must_use]
pub fn local_file_path(bare: &str, project_dir: &Path) -> Option<PathBuf> {
    if !bare.starts_with("file:") {
        return None;
    }
    let wanted = WantedLocalDependency { bare_specifier: bare.to_string(), injected: false };
    parse_local_scheme(&wanted, project_dir, project_dir, ParseOptions::default())
        .ok()
        .flatten()
        .map(|spec| spec.fetch_spec)
}

fn contains_path_sep(bare: &str) -> bool {
    bare.contains(std::path::MAIN_SEPARATOR) || bare.contains('/')
}
