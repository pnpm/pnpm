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
//!
//! The regular form qualifies the rewritten specifier with the target's
//! [`WorkspacePackageManifest::registry_prefix`]; a peer range stays a plain
//! semver range.

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
    workspace_packages: Option<&WorkspacePackageManifests>,
) -> Result<String, ReplaceWorkspaceProtocolError> {
    replace_workspace_spec(
        dep_name,
        dep_spec,
        dir,
        modules_dir,
        workspace_packages,
        RegistryPrefix::Carry,
    )
}

/// Whether a rewritten specifier carries the target's
/// [`WorkspacePackageManifest::registry_prefix`]. A peer range omits it: the
/// consumer matches the range against the installed version with semver, and
/// a prefix is no part of a semver range.
#[derive(Clone, Copy)]
enum RegistryPrefix {
    Carry,
    Omit,
}

fn replace_workspace_spec(
    dep_name: &str,
    dep_spec: &str,
    dir: &Path,
    modules_dir: Option<&Path>,
    workspace_packages: Option<&WorkspacePackageManifests>,
    registry_prefix: RegistryPrefix,
) -> Result<String, ReplaceWorkspaceProtocolError> {
    let Some(rest) = dep_spec.strip_prefix("workspace:") else {
        return Ok(dep_spec.to_string());
    };

    if let Some(parsed) = parse_version_alias_spec(rest) {
        let installed = installed_modules_dir(dir, modules_dir);
        let target_pkg_name = parsed.alias.unwrap_or(dep_name);
        let manifest = read_and_check_manifest(
            dep_name,
            target_pkg_name,
            &installed.join(dep_name),
            workspace_packages,
        )?;
        let token = match parsed.sentinel {
            Some('^') => "^",
            Some('~') => "~",
            _ => "",
        };
        return Ok(published_spec(dep_name, &manifest, token, registry_prefix));
    }

    if let Some(relative) = strip_workspace_relative_prefix(dep_spec) {
        let manifest =
            read_and_check_manifest(dep_name, dep_name, &dir.join(relative), workspace_packages)?;
        return Ok(published_spec(dep_name, &manifest, "", registry_prefix));
    }

    Ok(explicit_version_spec(dep_name, rest, workspace_packages, registry_prefix))
}

/// The published form of `workspace:<version>` and
/// `workspace:<alias>@<version>`. The version is published as written, so the
/// target's manifest is not read and a target that is not a workspace package
/// leaves the specifier plain; only the prefix of its registry is looked up.
fn explicit_version_spec(
    dep_name: &str,
    rest: &str,
    workspace_packages: Option<&WorkspacePackageManifests>,
    registry_prefix: RegistryPrefix,
) -> String {
    let (target_pkg_name, version) = match rest.rfind('@') {
        Some(0) => return format!("npm:{rest}"),
        Some(index) => (&rest[..index], &rest[index + 1..]),
        None => (dep_name, rest),
    };
    let prefix = match registry_prefix {
        RegistryPrefix::Carry => workspace_package_at(workspace_packages, target_pkg_name, version)
            .and_then(|pkg| pkg.registry_prefix.as_deref()),
        RegistryPrefix::Omit => None,
    };
    match (prefix, rest.contains('@')) {
        (Some(prefix), _) => format!("{prefix}:{rest}"),
        (None, true) => format!("npm:{rest}"),
        (None, false) => rest.to_string(),
    }
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
    workspace_packages: Option<&WorkspacePackageManifests>,
) -> Result<String, ReplaceWorkspaceProtocolError> {
    if !dep_spec.contains("workspace:") {
        return Ok(dep_spec.to_string());
    }
    match parsed_peer_spec(dep_spec) {
        Some(ParsedPeer::Alias(alias)) => return Ok(alias),
        Some(ParsedPeer::Relative) => {
            return replace_workspace_spec(
                dep_name,
                dep_spec,
                dir,
                modules_dir,
                workspace_packages,
                RegistryPrefix::Omit,
            );
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
    let manifest =
        read_and_check_manifest(dep_name, dep_name, &installed.join(dep_name), workspace_packages)?;
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

/// The workspace package `name` at `version`, or the one project of that name
/// when `version` is none of them. Several projects of the name and no match
/// select nothing: a guess would send a consumer to another project's
/// registry.
fn workspace_package_at<'a>(
    workspace_packages: Option<&'a WorkspacePackageManifests>,
    name: &str,
    version: &str,
) -> Option<&'a WorkspacePackageManifest> {
    let projects = workspace_packages?.get(name)?;
    if let Some(project) = projects
        .iter()
        .find(|project| project.version == version)
    {
        return Some(project);
    }
    match projects.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

/// The workspace package a `workspace:` specifier falls back on when the
/// dependency is not installed: the first project of the name with a version,
/// else the first, so that a missing version can be reported.
fn workspace_package_named<'a>(
    workspace_packages: Option<&'a WorkspacePackageManifests>,
    name: &str,
) -> Option<&'a WorkspacePackageManifest> {
    let projects = workspace_packages?.get(name)?;
    projects
        .iter()
        .find(|project| project.is_complete())
        .or_else(|| projects.first())
}

/// The directory a workspace dependency is installed under: the caller's, or
/// the project's own `node_modules`.
fn installed_modules_dir(dir: &Path, modules_dir: Option<&Path>) -> PathBuf {
    modules_dir.map_or_else(|| dir.join("node_modules"), Path::to_path_buf)
}

/// The specifier a published manifest records for a workspace dependency: the
/// resolved version, aliased when the package is published under another
/// name, and qualified with the registry prefix the manifest carries.
fn published_spec(
    dep_name: &str,
    manifest: &WorkspacePackageManifest,
    token: &str,
    registry_prefix: RegistryPrefix,
) -> String {
    let WorkspacePackageManifest {
        name,
        version,
        registry_prefix: prefix,
    } = manifest;
    let prefix = match registry_prefix {
        RegistryPrefix::Carry => prefix.as_deref(),
        RegistryPrefix::Omit => None,
    };
    match (prefix, name == dep_name) {
        (Some(prefix), true) => format!("{prefix}:{token}{version}"),
        (Some(prefix), false) => format!("{prefix}:{name}@{token}{version}"),
        (None, true) => format!("{token}{version}"),
        (None, false) => format!("npm:{name}@{token}{version}"),
    }
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
/// `target_pkg_name`. Surfaces the
/// `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL` error when the
/// dependency hasn't been installed yet or its manifest is incomplete.
fn read_and_check_manifest(
    dep_name: &str,
    target_pkg_name: &str,
    dependency_dir: &Path,
    workspace_packages: Option<&WorkspacePackageManifests>,
) -> Result<WorkspacePackageManifest, ReplaceWorkspaceProtocolError> {
    let manifest_from_dir = read_manifest_fields(dependency_dir)?;
    if let Some(manifest) = &manifest_from_dir
        && manifest.is_complete()
    {
        return Ok(manifest.clone().with_registry_prefix_of(workspace_packages));
    }

    let workspace_manifest = workspace_package_named(workspace_packages, target_pkg_name);
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
        registry_prefix: None,
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

/// The workspace's packages by name. A name holds one entry per project that
/// carries it: a workspace may hold several versions of one package.
pub type WorkspacePackageManifests = HashMap<String, Vec<WorkspacePackageManifest>>;

/// The fields the rewriters consult on the dependency's manifest, and the
/// registry prefix the dependency publishes under. `version` is empty when a
/// workspace package was found but its `package.json` has no `version` field,
/// so the caller can report that instead of claiming the dependency is not
/// installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePackageManifest {
    pub name: String,
    pub version: String,
    /// The named-registry prefix of the registry the package publishes to,
    /// when the workspace declares one for that registry. A published
    /// specifier for the package carries it (`work:1.0.0`), so a consumer
    /// records one lockfile key for the package whether it addresses the
    /// package through the prefix or through this manifest. `None` leaves the
    /// specifier plain.
    pub registry_prefix: Option<String>,
}

impl WorkspacePackageManifest {
    fn is_complete(&self) -> bool {
        !self.name.is_empty() && !self.version.is_empty()
    }

    /// A manifest read from `node_modules` says nothing about registries; the
    /// prefix is that of the workspace project of the same name and version.
    fn with_registry_prefix_of(
        mut self,
        workspace_packages: Option<&WorkspacePackageManifests>,
    ) -> Self {
        self.registry_prefix = workspace_package_at(workspace_packages, &self.name, &self.version)
            .and_then(|project| project.registry_prefix.clone());
        self
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
