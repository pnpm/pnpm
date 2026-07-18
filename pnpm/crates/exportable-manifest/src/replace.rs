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
    path::{Path, PathBuf},
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use serde_json::Value;

/// Error returned when the lookup against the dependency's installed
/// `package.json` fails. Carries the
/// `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL` error code; preserve the
/// public message so reporters that key off it keep matching.
#[derive(Debug, Display, Error, Diagnostic, Clone)]
#[display(
    "Cannot resolve workspace protocol of dependency \"{dep_name}\" \
     because this dependency is not installed. Try running \"pnpm install\"."
)]
#[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL))]
pub struct CannotResolveWorkspaceProtocolError {
    #[error(not(source))]
    pub dep_name: String,
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

/// Materialize a workspace dependency to an exact in-memory snapshot version.
/// `None` means either the specifier is not `workspace:` or its target is not
/// part of the snapshot plan, so the ordinary publish conversion should run.
pub fn replace_workspace_protocol_with_snapshot_version(
    workspace_versions: &HashMap<String, String>,
    dep_name: &str,
    dep_spec: &str,
    dir: &Path,
) -> Result<Option<String>, ReplaceWorkspaceProtocolError> {
    let Some(rest) = dep_spec.strip_prefix("workspace:") else {
        return Ok(None);
    };
    let target_name = if let Some(relative) = strip_workspace_relative_prefix(dep_spec) {
        read_and_check_manifest(dep_name, &dir.join(relative))?.name
    } else if let Some(separator) = rest.rfind('@').filter(|&separator| separator > 0) {
        rest[..separator].to_string()
    } else {
        dep_name.to_string()
    };
    let Some(version) = workspace_versions.get(&target_name) else {
        return Ok(None);
    };
    Ok(Some(if target_name == dep_name {
        version.clone()
    } else {
        format!("npm:{target_name}@{version}")
    }))
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
) -> Result<String, ReplaceWorkspaceProtocolError> {
    let Some(rest) = dep_spec.strip_prefix("workspace:") else {
        return Ok(dep_spec.to_string());
    };

    if let Some(parsed) = parse_version_alias_spec(rest) {
        let modules_dir_owned: PathBuf;
        let modules_dir = if let Some(path) = modules_dir {
            path
        } else {
            modules_dir_owned = dir.join("node_modules");
            &modules_dir_owned
        };
        let manifest = read_and_check_manifest(dep_name, &modules_dir.join(dep_name))?;
        let semver_range_token = match parsed.sentinel {
            Some('^') => "^",
            Some('~') => "~",
            _ => "",
        };
        if dep_name != manifest.name {
            return Ok(format!(
                "npm:{name}@{token}{version}",
                name = manifest.name,
                token = semver_range_token,
                version = manifest.version,
            ));
        }
        return Ok(format!(
            "{token}{version}",
            token = semver_range_token,
            version = manifest.version,
        ));
    }

    if let Some(relative) = strip_workspace_relative_prefix(dep_spec) {
        let manifest = read_and_check_manifest(dep_name, &dir.join(relative))?;
        if manifest.name == dep_name {
            return Ok(manifest.version);
        }
        return Ok(format!(
            "npm:{name}@{version}",
            name = manifest.name,
            version = manifest.version,
        ));
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
) -> Result<String, ReplaceWorkspaceProtocolError> {
    if !dep_spec.contains("workspace:") {
        return Ok(dep_spec.to_string());
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

    let modules_dir_owned: PathBuf;
    let modules_dir = if let Some(path) = modules_dir {
        path
    } else {
        modules_dir_owned = dir.join("node_modules");
        &modules_dir_owned
    };
    let manifest = read_and_check_manifest(dep_name, &modules_dir.join(dep_name))?;
    let token = if matched.range_group == "*" { "" } else { matched.range_group };

    let mut rewritten = String::with_capacity(dep_spec.len());
    rewritten.push_str(&dep_spec[..matched.start]);
    rewritten.push_str(token);
    rewritten.push_str(&manifest.version);
    rewritten.push_str(&dep_spec[matched.end..]);
    Ok(rewritten)
}

/// Read `<dependency_dir>/package.json` and verify the `name` / `version`
/// fields are present. Surfaces the
/// `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL` error when the
/// dependency hasn't been installed yet.
fn read_and_check_manifest(
    dep_name: &str,
    dependency_dir: &Path,
) -> Result<DependencyManifest, ReplaceWorkspaceProtocolError> {
    let value = match safe_read_package_json_from_dir(dependency_dir) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Err(ReplaceWorkspaceProtocolError::CannotResolve(
                CannotResolveWorkspaceProtocolError { dep_name: dep_name.to_string() },
            ));
        }
        Err(err) => return Err(ReplaceWorkspaceProtocolError::ReadManifest(err)),
    };
    let Some(name) = value.get("name").and_then(Value::as_str) else {
        return Err(ReplaceWorkspaceProtocolError::CannotResolve(
            CannotResolveWorkspaceProtocolError { dep_name: dep_name.to_string() },
        ));
    };
    let Some(version) = value.get("version").and_then(Value::as_str) else {
        return Err(ReplaceWorkspaceProtocolError::CannotResolve(
            CannotResolveWorkspaceProtocolError { dep_name: dep_name.to_string() },
        ));
    };
    Ok(DependencyManifest { name: name.to_string(), version: version.to_string() })
}

/// The two fields the rewriters consult on the dependency's manifest.
struct DependencyManifest {
    name: String,
    version: String,
}

/// Output of [`parse_version_alias_spec`]: the optional sentinel
/// character (`^`/`~`/`*`). The alias portion of the spec is not
/// captured here — it is implied by the spec shape and re-read from the
/// dependency manifest.
struct VersionAliasMatch {
    sentinel: Option<char>,
}

/// Parse the `workspace:` suffix of the form `(<alias>@)?[\^~*]?`.
///
/// Greedy backtracking on the alias means it spans up to (and
/// including) the **last** `@` in the suffix. Returns `None` when the
/// input has trailing characters past an optional `^`/`~`/`*` sentinel.
fn parse_version_alias_spec(after_protocol: &str) -> Option<VersionAliasMatch> {
    let after_alias = match after_protocol.rfind('@') {
        Some(idx) if idx >= 1 => &after_protocol[idx + 1..],
        _ => after_protocol,
    };
    let sentinel = match after_alias.chars().count() {
        0 => None,
        1 => {
            let first_char = after_alias.chars().next().expect("char count == 1");
            if matches!(first_char, '^' | '~' | '*') {
                Some(first_char)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(VersionAliasMatch { sentinel })
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
