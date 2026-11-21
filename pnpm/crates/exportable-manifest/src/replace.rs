//! Workspace-protocol rewriting for the exportable manifest.
//!
//! Two free functions:
//!
//! - [`replace_workspace_protocol`], the regular-dependency form.
//!   Resolves `workspace:` specs against the dependency's already-
//!   installed `package.json` in `node_modules`.
//! - [`replace_workspace_protocol_peer_dependency`], the
//!   peer-dependency form. Accepts the broader `>=`/`<=`/`>`/`<`
//!   comparators allowed in peer specs and rewrites every
//!   `workspace:` segment in place so a compound `a || workspace:>=`
//!   round-trips correctly.

use std::{
    collections::{
        HashMap,
        HashSet,
    },
    fmt,
    path::{
        Path,
        PathBuf,
    },
};

use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_package_manifest::{
    PackageManifestError,
    safe_read_project_manifest_from_dir,
};
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
        // A relative spec names its target by path, so the manifest at that
        // path is authoritative; the name-keyed workspace lookup stays a
        // fallback only, since `dep_name` may belong to another package.
        let manifest = read_and_check_manifest(
            dep_name,
            dep_name,
            &dir.join(relative),
            WorkspacePackageLookup { prefer_workspace_for: None, ..lookup },
        )?;
        return Ok(published_spec(dep_name, &manifest, ""));
    }

    if let Some(spec) = WorkspaceSpec::parse(dep_spec) {
        let target = spec.alias.as_deref().unwrap_or(dep_name);
        if let Some(renamed) = lookup.renamed_range(
            dep_name,
            target,
            &spec.version,
            &installed_modules_dir(dir, modules_dir),
        )? {
            return Ok(renamed);
        }
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
    if let Some(spec) = WorkspaceSpec::parse(dep_spec)
        && let Some(alias) = spec.alias.as_deref()
    {
        let range = match spec.version.as_str() {
            "^" | "~" | "" => "*",
            range => range,
        };
        return Ok(lookup
            .renamed_range(dep_name, alias, range, &installed_modules_dir(dir, modules_dir))?
            .unwrap_or_else(|| format!("npm:{alias}@{range}")));
    }
    if strip_workspace_relative_prefix(dep_spec).is_some() {
        return replace_workspace_protocol(dep_name, dep_spec, dir, modules_dir, lookup);
    }
    resolve_peer_range(dep_name, dep_spec, &installed_modules_dir(dir, modules_dir), lookup)
}

fn resolve_peer_range(
    dep_name: &str,
    dep_spec: &str,
    installed: &Path,
    lookup: WorkspacePackageLookup<'_>,
) -> Result<String, ReplaceWorkspaceProtocolError> {
    // Only the first `workspace:` occurrence is stripped. Rust's
    // `str::replace` is all-occurrence; use `replacen(_, _, 1)` so
    // compound peer specs like `^1.0.0 || workspace:>=1 || workspace:>=2`
    // keep the right behavior.
    let Some(matched) = find_workspace_peer_segment(dep_spec) else {
        return Ok(dep_spec.replacen("workspace:", "", 1));
    };

    if !matched.version.is_empty() {
        let range = dep_spec.replacen("workspace:", "", 1);
        return Ok(lookup.renamed_range(dep_name, dep_name, &range, installed)?.unwrap_or(range));
    }

    let manifest = read_and_check_manifest(dep_name, dep_name, &installed.join(dep_name), lookup)?;
    let token = if matched.range_group == "*" { "" } else { matched.range_group };

    let mut rewritten = String::with_capacity(dep_spec.len());
    rewritten.push_str(&dep_spec[..matched.start]);
    rewritten.push_str(token);
    rewritten.push_str(&manifest.version);
    rewritten.push_str(&dep_spec[matched.end..]);
    Ok(match &manifest.publish_name {
        Some(name) => registry_spec(dep_name, name, &rewritten),
        None => rewritten,
    })
}

/// The manifest a `workspace:` specifier resolves to, read with the same
/// precedence and fallbacks as [`replace_workspace_protocol`]. A target the
/// rewrite cannot use, because its manifest lacks a `name` or `version`, is
/// still returned as found. `None` when `dep_spec` is not a `workspace:`
/// specifier or names no package at all.
pub(crate) fn workspace_target_manifest(
    dep_name: &str,
    dep_spec: &str,
    dir: &Path,
    modules_dir: Option<&Path>,
    lookup: WorkspacePackageLookup<'_>,
) -> Result<Option<WorkspacePackageManifest>, ReplaceWorkspaceProtocolError> {
    let Some(workspace_spec) = WorkspaceSpec::parse(dep_spec) else {
        return Ok(None);
    };
    let (target_name, target_dir, lookup) = match strip_workspace_relative_prefix(dep_spec) {
        Some(relative) => (
            dep_name,
            dir.join(relative),
            WorkspacePackageLookup { prefer_workspace_for: None, ..lookup },
        ),
        None => (
            workspace_spec.alias.as_deref().unwrap_or(dep_name),
            installed_modules_dir(dir, modules_dir).join(dep_name),
            lookup,
        ),
    };
    lookup.resolve_manifest(target_name, &target_dir)
}

/// The directory a workspace dependency is installed under: the caller's, or
/// the project's own `node_modules`.
fn installed_modules_dir(dir: &Path, modules_dir: Option<&Path>) -> PathBuf {
    modules_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dir.join("node_modules"))
}

/// The specifier a published manifest records for a workspace dependency: the
/// resolved version, or an `npm:` alias when the package is published under
/// another name.
fn published_spec(dep_name: &str, manifest: &WorkspacePackageManifest, token: &str) -> String {
    let name = manifest.publish_name.as_deref().unwrap_or(&manifest.name);
    registry_spec(dep_name, name, &format!("{token}{}", manifest.version))
}

fn registry_spec(dep_name: &str, published_name: &str, range: &str) -> String {
    if published_name == dep_name {
        range.to_string()
    } else {
        format!("npm:{published_name}@{range}")
    }
}

/// Read `<dependency_dir>/package.json` and verify the `name` / `version`
/// fields are present, falling back to the workspace package named
/// `target_pkg_name`, or starting from it, when the lookup prefers the
/// workspace manifests. Surfaces the
/// `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL` error when the
/// dependency hasn't been installed yet or its manifest is incomplete.
fn read_and_check_manifest(
    dep_name: &str,
    target_pkg_name: &str,
    dependency_dir: &Path,
    lookup: WorkspacePackageLookup<'_>,
) -> Result<WorkspacePackageManifest, ReplaceWorkspaceProtocolError> {
    match lookup.resolve_manifest(target_pkg_name, dependency_dir)? {
        Some(manifest) if manifest.is_complete() => Ok(manifest),
        found => {
            Err(ReplaceWorkspaceProtocolError::CannotResolve(cannot_resolve_error(dep_name, found)))
        }
    }
}

/// Read the dependency at `dir` in project-manifest precedence order.
fn read_manifest_fields(
    dir: &Path,
) -> Result<Option<WorkspacePackageManifest>, ReplaceWorkspaceProtocolError> {
    let value = safe_read_project_manifest_from_dir(dir)
        .map_err(ReplaceWorkspaceProtocolError::ReadManifest)?;
    Ok(value.as_ref().map(WorkspacePackageManifest::from_manifest))
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

/// The fields used to publish a workspace dependency.
/// `version` is empty when a workspace package was found but its
/// `package.json` has no `version` field, so the caller can report that
/// instead of claiming the dependency is not installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePackageManifest {
    pub name: String,
    pub publish_name: Option<String>,
    pub version: String,
    /// The package's `"private": true`.
    pub private: bool,
}

/// How a workspace dependency's manifest is resolved at publish time: the
/// packages discovered in the workspace, and which of them take precedence
/// over the copies installed in `node_modules`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorkspacePackageLookup<'a> {
    /// Workspace packages by manifest name, consulted when the dependency
    /// is not installed, or first, when [`Self::prefer_workspace_for`]
    /// names it.
    pub packages: Option<&'a HashMap<String, WorkspacePackageManifest>>,
    /// Resolve from the workspace manifests before the installed copies,
    /// but only for the packages named here. `publish --new-version` sets
    /// this to the packages whose version it rewrote: those manifests carry
    /// the new version while `node_modules` can still hold the pre-bump
    /// copies, and every other dependency keeps the installed-copy-first
    /// resolution a plain publish uses. Applies only to specs resolved by
    /// name; a relative-path spec always reads the manifest at its target
    /// path first.
    pub prefer_workspace_for: Option<&'a HashSet<String>>,
}

impl WorkspacePackageLookup<'_> {
    fn resolve_manifest(
        self,
        target_pkg_name: &str,
        dependency_dir: &Path,
    ) -> Result<Option<WorkspacePackageManifest>, ReplaceWorkspaceProtocolError> {
        let workspace_manifest = self.packages.and_then(|pkgs| pkgs.get(target_pkg_name));
        if let Some(preferred) = self.prefer_workspace_for
            && preferred.contains(target_pkg_name)
            && let Some(manifest) = workspace_manifest
            && manifest.is_complete()
        {
            return Ok(Some(manifest.clone()));
        }

        let manifest_from_dir = read_manifest_fields(dependency_dir)?;
        if let Some(manifest) = &manifest_from_dir
            && manifest.is_complete()
        {
            return Ok(Some(manifest.clone()));
        }

        if let Some(manifest) = workspace_manifest
            && manifest.is_complete()
        {
            return Ok(Some(manifest.clone()));
        }

        Ok(match manifest_from_dir {
            Some(manifest) if !manifest.name.is_empty() || !manifest.version.is_empty() => {
                Some(manifest)
            }
            manifest_from_dir => workspace_manifest.cloned().or(manifest_from_dir),
        })
    }

    fn renamed_range(
        self,
        dep_name: &str,
        target: &str,
        range: &str,
        modules_dir: &Path,
    ) -> Result<Option<String>, ReplaceWorkspaceProtocolError> {
        let manifest = self.resolve_manifest(target, &modules_dir.join(dep_name))?;
        Ok(manifest
            .and_then(|manifest| manifest.publish_name)
            .map(|name| registry_spec(dep_name, &name, range)))
    }
}

impl<'a> From<Option<&'a HashMap<String, WorkspacePackageManifest>>>
    for WorkspacePackageLookup<'a>
{
    fn from(packages: Option<&'a HashMap<String, WorkspacePackageManifest>>) -> Self {
        WorkspacePackageLookup { packages, prefer_workspace_for: None }
    }
}

impl WorkspacePackageManifest {
    #[must_use]
    pub fn from_manifest(manifest: &Value) -> Self {
        let name = manifest
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Self {
            name: name.to_string(),
            publish_name: pnpm_package_manifest::published_name(manifest)
                .filter(|renamed| *renamed != name)
                .map(str::to_owned),
            version: manifest
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            private: manifest.get("private") == Some(&Value::Bool(true)),
        }
    }

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
/// the byte offset one past the consumed region, or `pos` when the
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
