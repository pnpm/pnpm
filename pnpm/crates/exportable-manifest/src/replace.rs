//! Workspace-protocol rewriting for the exportable manifest.
//!
//! Two free functions:
//!
//! - [`replace_workspace_protocol`] — the regular-dependency form.
//!   Resolves `workspace:` specs against the dependency's already-
//!   installed `package.json` in `node_modules`.
//! - [`replace_workspace_protocol_peer_dependency`] — the
//!   peer-dependency form. Accepts the broader `>=`/`<=`/`>`/`<`
//!   comparators allowed in peer specs and rewrites every
//!   `workspace:` segment in place so a compound `a || workspace:>=`
//!   round-trips correctly.

use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use pnpm_workspace_spec::WorkspaceSpec;
use serde_json::Value;

/// Error returned when the lookup against the dependency's installed
/// `package.json` fails. Carries the
/// `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL` error code.
#[derive(Debug, Display, Error, Clone)]
#[display(r#"Cannot resolve workspace protocol of dependency "{dep_name}" because {reason}"#)]
pub struct CannotResolveWorkspaceProtocolError {
    #[error(not(source))]
    pub dep_name: String,
    pub package_name: String,
    pub reason: CannotResolveReason,
}

impl Diagnostic for CannotResolveWorkspaceProtocolError {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new("ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL"))
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        match self.reason {
            CannotResolveReason::MissingVersion => Some(Box::new(format!(
                r#"Add a "version" field to the package.json of "{}"."#,
                self.package_name,
            ))),
            CannotResolveReason::MissingName | CannotResolveReason::NotInstalled => None,
        }
    }
}

/// Why a `workspace:` specifier could not be resolved to a published
/// version. `MissingVersion` / `MissingName` mean the package was found
/// but its manifest is incomplete, which is not the same as "not
/// installed" and must not be reported as such.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq)]
pub enum CannotResolveReason {
    #[display(r#"this dependency is not installed. Try running "pnpm install"."#)]
    NotInstalled,
    #[display(r#"its package.json has no "version" field."#)]
    MissingVersion,
    #[display(r#"its package.json has no "name" field."#)]
    MissingName,
}

/// Error envelope for both rewrite helpers.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ReplaceWorkspaceProtocolError {
    /// The dependency's directory was found but the `package.json`
    /// lacked one of the required fields. Most common reason: the
    /// project hasn't been installed yet.
    #[diagnostic(transparent)]
    CannotResolve(#[error(source)] CannotResolveWorkspaceProtocolError),

    /// Reading `<dep>/package.json` itself failed (malformed JSON, IO
    /// error other than ENOENT, ...). Propagated so the caller can
    /// surface the underlying cause.
    ReadManifest(#[error(source)] PackageManifestError),
}

/// Rewrites a single `dependencies` / `devDependencies` /
/// `optionalDependencies` value at publish time.
///
/// Returns `dep_spec` unchanged when it doesn't start with `workspace:`
/// so the caller can fold the helper into a generic per-field rewrite
/// without branching on the protocol.
pub fn replace_workspace_protocol(
    dep_name: &str,
    dep_spec: &str,
    dir: &Path,
    modules_dir: Option<&Path>,
    lookup: WorkspacePackageLookup<'_>,
) -> Result<String, ReplaceWorkspaceProtocolError> {
    let Some(rest) = dep_spec.strip_prefix("workspace:") else {
        return Ok(dep_spec.to_string());
    };

    if let Some(parsed) = parse_version_alias_spec(rest) {
        let installed = installed_modules_dir(dir, modules_dir);
        let target_pkg_name = parsed.alias.unwrap_or(dep_name);
        let manifest =
            read_and_check_manifest(dep_name, target_pkg_name, &installed.join(dep_name), lookup)?;
        let token = match parsed.sentinel {
            Some('^') => "^",
            Some('~') => "~",
            _ => "",
        };
        return Ok(published_spec(dep_name, &manifest, token));
    }

    if let Some(relative) = strip_workspace_relative_prefix(dep_spec) {
        let manifest = read_and_check_manifest(dep_name, dep_name, &dir.join(relative), lookup)?;
        return Ok(published_spec(dep_name, &manifest, ""));
    }

    if rest.contains('@') {
        return Ok(format!("npm:{rest}"));
    }
    Ok(rest.to_string())
}

/// Rewrites a `peerDependencies` value at publish time.
///
/// `peerDependencies` allows compound ranges (`workspace:>= || ^3.9.0`),
/// so this helper accepts the broader comparator set (`>=`, `<=`, `>`,
/// `<` alongside `^`, `~`, `*`) and rewrites every `workspace:`
/// segment in place rather than swapping the whole string.
pub fn replace_workspace_protocol_peer_dependency(
    dep_name: &str,
    dep_spec: &str,
    dir: &Path,
    modules_dir: Option<&Path>,
    lookup: WorkspacePackageLookup<'_>,
) -> Result<String, ReplaceWorkspaceProtocolError> {
    if !dep_spec.contains("workspace:") {
        return Ok(dep_spec.to_string());
    }
    match parsed_peer_spec(dep_spec) {
        Some(ParsedPeer::Alias(alias)) => return Ok(alias),
        Some(ParsedPeer::Relative) => {
            return replace_workspace_protocol(dep_name, dep_spec, dir, modules_dir, lookup);
        }
        None => {}
    }
    // Only the first `workspace:` occurrence is stripped. Rust's
    // `str::replace` is all-occurrence; use `replacen(_, _, 1)` so
    // compound peer specs like `^1.0.0 || workspace:>=1 || workspace:>=2`
    // keep the right behavior.
    let Some(matched) = find_workspace_peer_segment(dep_spec) else {
        return Ok(dep_spec.replacen("workspace:", "", 1));
    };

    if !matched.version.is_empty() {
        return Ok(dep_spec.replacen("workspace:", "", 1));
    }

    let installed = installed_modules_dir(dir, modules_dir);
    let manifest = read_and_check_manifest(dep_name, dep_name, &installed.join(dep_name), lookup)?;
    let token = if matched.range_group == "*" { "" } else { matched.range_group };

    let mut rewritten = String::with_capacity(dep_spec.len());
    rewritten.push_str(&dep_spec[..matched.start]);
    rewritten.push_str(token);
    rewritten.push_str(&manifest.version);
    rewritten.push_str(&dep_spec[matched.end..]);
    Ok(rewritten)
}

/// What a `workspace:` peer specifier parses as, when it parses as one whole.
enum ParsedPeer {
    /// An `npm:` alias the published manifest records verbatim.
    Alias(String),
    /// A relative path, which the ordinary rewrite resolves.
    Relative,
}

fn parsed_peer_spec(dep_spec: &str) -> Option<ParsedPeer> {
    let workspace_spec = WorkspaceSpec::parse(dep_spec)?;
    if let Some(alias) = workspace_spec.alias.as_deref() {
        return Some(ParsedPeer::Alias(aliased_peer_spec(alias, &workspace_spec.version)));
    }
    let relative =
        workspace_spec.version.starts_with("./") || workspace_spec.version.starts_with("../");
    relative.then_some(ParsedPeer::Relative)
}

/// The directory a workspace dependency is installed under: the caller's, or
/// the project's own `node_modules`.
fn installed_modules_dir(dir: &Path, modules_dir: Option<&Path>) -> PathBuf {
    modules_dir.map_or_else(|| dir.join("node_modules"), Path::to_path_buf)
}

/// The specifier a published manifest records for a workspace dependency: the
/// resolved version, or an `npm:` alias when the package is published under
/// another name.
fn published_spec(dep_name: &str, manifest: &WorkspacePackageManifest, token: &str) -> String {
    if manifest.name == dep_name {
        return format!("{token}{version}", version = manifest.version);
    }
    format!("npm:{name}@{token}{version}", name = manifest.name, version = manifest.version)
}

/// An aliased peer keeps its alias; a range sentinel with no version behind it
/// widens to `*`, which is what a peer range without a resolved version means.
fn aliased_peer_spec(alias: &str, version: &str) -> String {
    let version =
        if version == "^" || version == "~" || version.is_empty() { "*" } else { version };
    format!("npm:{alias}@{version}")
}

/// Read `<dependency_dir>/package.json` and verify the `name` / `version`
/// fields are present, falling back to the workspace package named
/// `target_pkg_name` — or starting from it, when the lookup prefers the
/// workspace manifests. Surfaces the
/// `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL` error when the
/// dependency hasn't been installed yet or its manifest is incomplete.
fn read_and_check_manifest(
    dep_name: &str,
    target_pkg_name: &str,
    dependency_dir: &Path,
    lookup: WorkspacePackageLookup<'_>,
) -> Result<WorkspacePackageManifest, ReplaceWorkspaceProtocolError> {
    let workspace_manifest = lookup.packages.and_then(|pkgs| pkgs.get(target_pkg_name));
    if lookup.prefer_workspace
        && let Some(manifest) = workspace_manifest
        && manifest.is_complete()
    {
        return Ok(manifest.clone());
    }

    let manifest_from_dir = read_manifest_fields(dependency_dir)?;
    if let Some(manifest) = &manifest_from_dir
        && manifest.is_complete()
    {
        return Ok(manifest.clone());
    }

    if let Some(manifest) = workspace_manifest
        && manifest.is_complete()
    {
        return Ok(manifest.clone());
    }

    let found = match manifest_from_dir {
        Some(manifest) if !manifest.name.is_empty() || !manifest.version.is_empty() => {
            Some(manifest)
        }
        manifest_from_dir => workspace_manifest.cloned().or(manifest_from_dir),
    };
    Err(ReplaceWorkspaceProtocolError::CannotResolve(cannot_resolve_error(dep_name, found)))
}

/// The `name` / `version` fields of `<dir>/package.json`, each empty when
/// absent, or `None` when the file doesn't exist.
fn read_manifest_fields(
    dir: &Path,
) -> Result<Option<WorkspacePackageManifest>, ReplaceWorkspaceProtocolError> {
    let value =
        safe_read_package_json_from_dir(dir).map_err(ReplaceWorkspaceProtocolError::ReadManifest)?;
    let field = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok(value.map(|value| WorkspacePackageManifest {
        name: field(&value, "name"),
        version: field(&value, "version"),
    }))
}

/// Classify an unresolvable dependency by the incomplete manifest that was
/// found for it, if any.
fn cannot_resolve_error(
    dep_name: &str,
    found: Option<WorkspacePackageManifest>,
) -> CannotResolveWorkspaceProtocolError {
    let (package_name, reason) = match found {
        Some(manifest) if !manifest.name.is_empty() => {
            (manifest.name, CannotResolveReason::MissingVersion)
        }
        Some(_) => (dep_name.to_string(), CannotResolveReason::MissingName),
        None => (dep_name.to_string(), CannotResolveReason::NotInstalled),
    };
    CannotResolveWorkspaceProtocolError { dep_name: dep_name.to_string(), package_name, reason }
}

/// The two fields the rewriters consult on the dependency's manifest.
/// `version` is empty when a workspace package was found but its
/// `package.json` has no `version` field, so the caller can report that
/// instead of claiming the dependency is not installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePackageManifest {
    pub name: String,
    pub version: String,
}

/// How a workspace dependency's manifest is resolved at publish time: the
/// packages discovered in the workspace, and whether they take precedence
/// over the copies installed in `node_modules`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorkspacePackageLookup<'a> {
    /// Workspace packages by manifest name, consulted when the dependency
    /// is not installed — or first, when [`Self::prefer_workspace`] is set.
    pub packages: Option<&'a HashMap<String, WorkspacePackageManifest>>,
    /// Resolve from the workspace manifests before the installed copies.
    /// Recursive `publish --new-version` sets this: it rewrites the
    /// workspace manifests before packing, while `node_modules` can still
    /// hold the pre-bump copies.
    pub prefer_workspace: bool,
}

impl<'a> From<Option<&'a HashMap<String, WorkspacePackageManifest>>>
    for WorkspacePackageLookup<'a>
{
    fn from(packages: Option<&'a HashMap<String, WorkspacePackageManifest>>) -> Self {
        WorkspacePackageLookup { packages, prefer_workspace: false }
    }
}

impl WorkspacePackageManifest {
    fn is_complete(&self) -> bool {
        !self.name.is_empty() && !self.version.is_empty()
    }
}

/// Output of [`parse_version_alias_spec`]: the optional sentinel
/// character (`^`/`~`/`*`) and optional alias.
struct VersionAliasMatch<'a> {
    alias: Option<&'a str>,
    sentinel: Option<char>,
}

/// Parse the `workspace:` suffix of the form `(<alias>@)?[\^~*]?`.
///
/// Greedy backtracking on the alias means it spans up to (and
/// including) the **last** `@` in the suffix. Returns `None` when the
/// input has trailing characters past an optional `^`/`~`/`*` sentinel.
fn parse_version_alias_spec(after_protocol: &str) -> Option<VersionAliasMatch<'_>> {
    let (alias, after_alias) = match after_protocol.rfind('@') {
        Some(idx) if idx >= 1 => (Some(&after_protocol[..idx]), &after_protocol[idx + 1..]),
        _ => (None, after_protocol),
    };
    let sentinel = match after_alias.chars().count() {
        0 => None,
        1 => {
            let first_char = after_alias
                .chars()
                .next()
                .expect("char count == 1");
            if matches!(first_char, '^' | '~' | '*') {
                Some(first_char)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(VersionAliasMatch { alias, sentinel })
}

/// Strip the `workspace:` prefix and return the path portion of a
/// relative `workspace:./` or `workspace:../` spec.
fn strip_workspace_relative_prefix(dep_spec: &str) -> Option<&str> {
    if dep_spec.starts_with("workspace:./") || dep_spec.starts_with("workspace:../") {
        return Some(&dep_spec["workspace:".len()..]);
    }
    None
}

/// One `workspace:`-led peer segment of the form
/// `workspace:([\^~*]|>=|>|<=|<)?((\d+|[xX*])(\.(\d+|[xX*])){0,2})?`.
struct WorkspacePeerSegment<'a> {
    /// Byte offset of the leading `workspace:` in the input.
    start: usize,
    /// Byte offset one past the end of the matched region.
    end: usize,
    /// The semver-range comparator, or the empty string when no
    /// comparator preceded the version component.
    range_group: &'a str,
    /// The version component, if any. Empty string when no version
    /// component followed the comparator.
    version: &'a str,
}

/// Locate the first `workspace:`-led peer segment in `spec` and return
/// the slice information [`replace_workspace_protocol_peer_dependency`]
/// needs to rewrite it in place.
fn find_workspace_peer_segment(spec: &str) -> Option<WorkspacePeerSegment<'_>> {
    let start = spec.find("workspace:")?;
    let after = start + "workspace:".len();
    let bytes = spec.as_bytes();

    let range_len = parse_peer_range_comparator(bytes, after);
    let comparator_end = after + range_len;
    let version_end = parse_peer_version(bytes, comparator_end);

    Some(WorkspacePeerSegment {
        start,
        end: version_end,
        range_group: &spec[after..comparator_end],
        version: &spec[comparator_end..version_end],
    })
}

/// Parse the leading comparator (`>=`, `<=`, `>`, `<`, `^`, `~`, `*`)
/// starting at `pos`. Returns the byte length of the comparator, or
/// zero when no comparator is present.
fn parse_peer_range_comparator(bytes: &[u8], pos: usize) -> usize {
    match (bytes.get(pos), bytes.get(pos + 1)) {
        (Some(b'>' | b'<'), Some(b'=')) => 2,
        (Some(b'>' | b'<' | b'^' | b'~' | b'*'), _) => 1,
        _ => 0,
    }
}

/// Consume `(\d+|[xX*])(\.(\d+|[xX*])){0,2}` starting at `pos`. Returns
/// the byte offset one past the consumed region — or `pos` when the
/// first character isn't a valid part.
fn parse_peer_version(bytes: &[u8], pos: usize) -> usize {
    let Some(end) = parse_peer_version_part(bytes, pos) else {
        return pos;
    };
    let mut cur = end;
    for _ in 0..2 {
        if bytes.get(cur) != Some(&b'.') {
            break;
        }
        let Some(next) = parse_peer_version_part(bytes, cur + 1) else {
            break;
        };
        cur = next;
    }
    cur
}

/// Consume one part of the version regex (`\d+` or one of `x`/`X`/`*`).
fn parse_peer_version_part(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start)? {
        b'x' | b'X' | b'*' => Some(start + 1),
        b if b.is_ascii_digit() => {
            let mut end = start + 1;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            Some(end)
        }
        _ => None,
    }
}
