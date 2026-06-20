//! Pacquet port of pnpm's
//! [`parseBareSpecifier`](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/npm-resolver/src/parseBareSpecifier.ts).
//!
//! Routes a raw bare specifier (`"^1.0.0"`, `"latest"`,
//! `"npm:lodash@^4"`, `"https://registry.npmjs.org/foo/-/foo-1.0.0.tgz"`)
//! to a [`RegistryPackageSpec`] the npm picker can consume, or `None`
//! when no npm-shaped interpretation applies — that's the signal to the
//! resolver chain to try the next resolver in the chain.
//!
//! The sibling [`parse_jsr_specifier_to_registry_package_spec`] routes
//! `jsr:` specifiers the same way: parser-package output through the
//! version-selector classifier, then folded into an npm-shaped spec
//! the picker can drive against the `@jsr` registry.

use std::collections::HashSet;

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pacquet_resolving_jsr_specifier_parser::{ParseJsrSpecifierError, parse_jsr_specifier};
use reqwest::Url;

use crate::pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType};

/// Discriminator + normalized form produced by [`get_version_selector_type`].
pub(crate) struct VersionSelectorMatch {
    pub(crate) spec_type: RegistryPackageSpecType,
    pub(crate) normalized: String,
}

/// Parse an npm-style `(bare_specifier, alias, default_tag, registry)`
/// into a [`RegistryPackageSpec`].
///
/// Returns `None` for any specifier the npm resolver doesn't claim
/// (git URLs, workspace protocol, catalog protocol, etc.), so the
/// resolver chain falls through to the next entry.
pub fn parse_bare_specifier(
    bare_specifier: &str,
    alias: Option<&str>,
    default_tag: &str,
    registry: &str,
) -> Option<RegistryPackageSpec> {
    let mut name: Option<String> = alias.map(str::to_string);
    let mut bare = bare_specifier.to_string();

    if let Some(rest) = bare.strip_prefix("npm:") {
        bare = rest.to_string();

        let alias_str = alias;
        if let Some(a) = alias_str
            && !a.is_empty()
            && Range::parse(&bare).is_ok()
        {
            name = Some(a.to_string());
        } else {
            // Last `@` discriminates `name@version`.
            let last_at =
                bare.bytes().enumerate().rev().find_map(|(i, b)| (b == b'@').then_some(i));
            match last_at {
                Some(idx) if idx >= 1 => {
                    name = Some(bare[..idx].to_string());
                    bare = bare[idx + 1..].to_string();
                }
                _ => {
                    name = Some(bare.clone());
                    bare = default_tag.to_string();
                }
            }
        }
    }

    if let Some(name) = name.as_ref()
        && !name.is_empty()
        && let Some(selector) = get_version_selector_type(&bare)
    {
        return Some(RegistryPackageSpec {
            name: name.clone(),
            fetch_spec: selector.normalized,
            spec_type: selector.spec_type,
            normalized_bare_specifier: None,
        });
    }

    if bare.starts_with(registry)
        && let Some(pkg) = parse_npm_tarball_url(&bare)
    {
        return Some(RegistryPackageSpec {
            name: pkg.name,
            fetch_spec: pkg.version,
            spec_type: RegistryPackageSpecType::Version,
            normalized_bare_specifier: Some(bare),
        });
    }

    None
}

/// JSR-specifier counterpart of [`RegistryPackageSpec`]. Carries the
/// JSR-style scoped name alongside the npm-shaped fields so the
/// resolver can record the dependency under its JSR alias while
/// driving the picker against the `@jsr` registry.
///
/// Mirrors upstream's `JsrRegistryPackageSpec`
/// ([source](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/npm-resolver/src/parseBareSpecifier.ts#L64-L66)).
#[derive(Debug, Clone)]
pub struct JsrRegistryPackageSpec {
    pub spec: RegistryPackageSpec,
    pub jsr_pkg_name: String,
}

/// Parse a `jsr:` specifier into a picker-ready
/// [`JsrRegistryPackageSpec`].
///
/// Defers the `jsr:` syntax to the
/// [`pacquet_resolving_jsr_specifier_parser`] crate, then runs the
/// version-selector classifier on the parsed selector (falling back
/// to `default_tag` when the specifier omits one). Returns
/// `Ok(None)` for any non-`jsr:` specifier so the caller can fall
/// through to the npm bare-specifier parser.
///
/// Mirrors upstream's
/// [`parseJsrSpecifierToRegistryPackageSpec`](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/npm-resolver/src/parseBareSpecifier.ts#L68-L85).
pub fn parse_jsr_specifier_to_registry_package_spec(
    raw_specifier: &str,
    alias: Option<&str>,
    default_tag: &str,
) -> Result<Option<JsrRegistryPackageSpec>, ParseJsrSpecifierError> {
    let Some(spec) = parse_jsr_specifier(raw_specifier, alias)? else {
        return Ok(None);
    };

    let selector_input = spec.version_selector.as_deref().unwrap_or(default_tag);
    let Some(selector) = get_version_selector_type(selector_input) else {
        return Ok(None);
    };

    Ok(Some(JsrRegistryPackageSpec {
        spec: RegistryPackageSpec {
            name: spec.npm_pkg_name,
            fetch_spec: selector.normalized,
            spec_type: selector.spec_type,
            normalized_bare_specifier: None,
        },
        jsr_pkg_name: spec.jsr_pkg_name,
    }))
}

/// Named-registry counterpart of [`RegistryPackageSpec`]. Carries the
/// matched alias alongside the npm-shaped fields so the resolver can
/// route the metadata fetch to the configured registry URL while still
/// driving the picker against an npm-shaped spec.
///
/// Mirrors upstream's `NamedRegistryPackageSpec`
/// ([source](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/parseBareSpecifier.ts#L91-L93)).
#[derive(Debug, Clone)]
pub struct NamedRegistryPackageSpec {
    pub spec: RegistryPackageSpec,
    /// The alias that was matched (e.g. `gh`, or a user-defined name).
    /// Reported back to the caller so the resolver can look the URL up
    /// in its merged named-registries map.
    pub registry_name: String,
}

/// Failure from [`parse_named_registry_specifier_to_registry_package_spec`].
///
/// Mirrors upstream's
/// [`ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/parseBareSpecifier.ts#L131-L136).
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseNamedRegistrySpecifierError {
    /// Scope without a `/<name>` segment. Always a configuration bug,
    /// refused so the user gets an immediate, actionable error instead
    /// of a confusing downstream 404.
    #[display("The package name '{pkg_name}' in named registry '{registry_name}:' is invalid")]
    #[diagnostic(code(ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME))]
    InvalidPackageName {
        registry_name: String,
        #[error(not(source))]
        pkg_name: String,
    },
}

/// Parse a named-registry specifier of the shape `<alias>:<body>` into
/// a [`NamedRegistryPackageSpec`].
///
/// Returns `Ok(None)` for any specifier that does not use one of the
/// configured aliases (no colon, alias unknown, body unparsable) so
/// the caller can fall through to other resolvers. Errors only for
/// recoverable-by-the-user input — a scoped name with no `/<name>`
/// segment.
///
/// Supported shapes:
/// - `<alias>:[@<owner>/]<name>[@<version_selector>]`
/// - `<alias>:<version_selector>` paired with a package alias
///
/// Mirrors upstream's
/// [`parseNamedRegistrySpecifierToRegistryPackageSpec`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/parseBareSpecifier.ts#L101-L164).
pub fn parse_named_registry_specifier_to_registry_package_spec(
    raw_specifier: &str,
    known_registry_names: &HashSet<String>,
    package_alias: Option<&str>,
    default_tag: &str,
) -> Result<Option<NamedRegistryPackageSpec>, ParseNamedRegistrySpecifierError> {
    let Some(colon) = raw_specifier.find(':') else {
        return Ok(None);
    };
    if colon == 0 {
        return Ok(None);
    }
    let registry_name = &raw_specifier[..colon];
    if !known_registry_names.contains(registry_name) {
        return Ok(None);
    }

    let body = &raw_specifier[colon + 1..];
    let pkg_name: String;
    let version_selector: Option<String>;

    if Range::parse(body).is_ok() {
        let Some(alias) = package_alias.filter(|alias| !alias.is_empty()) else {
            return Ok(None);
        };
        pkg_name = alias.to_string();
        version_selector = Some(body.to_string());
    } else if body.starts_with('@') {
        // `<alias>:@<owner>/<name>[@<version_selector>]` — scoped package.
        let last_at = body.rfind('@').expect("body starts with '@'");
        let (name_part, ver_part) = if last_at == 0 {
            (body, None)
        } else {
            (&body[..last_at], Some(body[last_at + 1..].to_string()))
        };
        if !name_part.contains('/') || name_part.ends_with('/') {
            return Err(ParseNamedRegistrySpecifierError::InvalidPackageName {
                registry_name: registry_name.to_string(),
                pkg_name: name_part.to_string(),
            });
        }
        pkg_name = name_part.to_string();
        version_selector = ver_part;
    } else if package_alias.is_some_and(|alias| alias.starts_with('@')) {
        // `<alias>:<tag>` paired with a scoped alias — body is a
        // version selector (tag/dist-tag). Mirrors GitHub Packages,
        // where the package is always scoped and a bare body is a tag.
        pkg_name = package_alias.expect("checked above").to_string();
        version_selector = Some(body.to_string());
    } else {
        // `<alias>:<name>[@<version_selector>]` — unscoped package in body.
        let last_at = body.bytes().enumerate().rev().find_map(|(i, b)| (b == b'@').then_some(i));
        let (name_part, ver_part) = match last_at {
            Some(idx) if idx >= 1 => (&body[..idx], Some(body[idx + 1..].to_string())),
            _ => (body, None),
        };
        if name_part.is_empty() {
            return Ok(None);
        }
        pkg_name = name_part.to_string();
        version_selector = ver_part;
    }

    let selector_input = version_selector.as_deref().unwrap_or(default_tag);
    let Some(selector) = get_version_selector_type(selector_input) else {
        return Ok(None);
    };

    Ok(Some(NamedRegistryPackageSpec {
        spec: RegistryPackageSpec {
            name: pkg_name,
            fetch_spec: selector.normalized,
            spec_type: selector.spec_type,
            normalized_bare_specifier: None,
        },
        registry_name: registry_name.to_string(),
    }))
}

/// Discriminate between an exact version, a semver range, and a
/// dist-tag, returning the normalized form alongside the discriminator.
/// Mirrors npm's
/// [`version-selector-type`](https://github.com/pnpm/version-selector-type/blob/v3.0.0/index.js):
/// version first, range second, tag last. Returns `None` only when the
/// selector contains characters that JS's `encodeURIComponent` would
/// escape (i.e. not a valid npm tag).
pub(crate) fn get_version_selector_type(selector: &str) -> Option<VersionSelectorMatch> {
    if let Ok(version) = Version::parse(selector) {
        return Some(VersionSelectorMatch {
            spec_type: RegistryPackageSpecType::Version,
            normalized: version.to_string(),
        });
    }
    if Range::parse(selector).is_ok() {
        return Some(VersionSelectorMatch {
            spec_type: RegistryPackageSpecType::Range,
            normalized: selector.to_string(),
        });
    }
    if is_valid_dist_tag(selector) {
        return Some(VersionSelectorMatch {
            spec_type: RegistryPackageSpecType::Tag,
            normalized: selector.to_string(),
        });
    }
    None
}

/// Mirrors JS's `encodeURIComponent(s) === s` check upstream uses to
/// reject anything not safe to embed in a URL segment. The unreserved
/// set is `A-Z a-z 0-9 - _ . ! ~ * ' ( )` — anything else (including
/// `/`, `:`, spaces) bumps the candidate out of the tag bucket so
/// protocol-prefixed specifiers fall through to the next resolver.
fn is_valid_dist_tag(selector: &str) -> bool {
    selector.bytes().all(|byte| {
        matches!(byte,
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
    })
}

struct NpmTarballUrl {
    name: String,
    version: String,
}

/// Pacquet port of npm's
/// [`parse-npm-tarball-url`](https://github.com/zkochan/packages/blob/main/parse-npm-tarball-url/src/index.ts).
/// Extracts `(name, version)` from a URL like
/// `https://registry.npmjs.org/foo/-/foo-1.0.0.tgz`. Returns `None`
/// when the URL doesn't fit the npm tarball layout or the trailing
/// version segment isn't valid semver.
fn parse_npm_tarball_url(url: &str) -> Option<NpmTarballUrl> {
    let parsed = Url::parse(url).ok()?;
    parsed.host_str()?;
    let path = parsed.path();
    if path.is_empty() {
        return None;
    }
    let parts: Vec<&str> = path.split("/-/").collect();
    if parts.len() != 2 {
        return None;
    }
    let raw_name = parts[0].strip_prefix('/').unwrap_or(parts[0]);
    if raw_name.is_empty() {
        return None;
    }
    let name = percent_decode_str(raw_name);
    if name.is_empty() {
        return None;
    }
    let path_with_no_ext = parts[1].strip_suffix(".tgz").unwrap_or(parts[1]);
    // The tarball filename always starts with the scopeless name
    // followed by `-`. Anchor on that prefix instead of slicing by
    // length so a registry that returns `foo/-/bar-1.0.0.tgz` (name
    // mismatch) doesn't get accepted and mapped to the wrong package.
    let scopeless_name = name.rsplit('/').next().unwrap_or(name.as_str());
    let version =
        path_with_no_ext.strip_prefix(scopeless_name).and_then(|rest| rest.strip_prefix('-'))?;
    Version::parse(version).ok()?;
    Some(NpmTarballUrl { name, version: version.to_string() })
}

/// Percent-decode a URL path segment. Matches JS's `decodeURIComponent`
/// for the byte ranges that show up in npm tarball URLs (the only
/// caller). Invalid escapes pass through unchanged, mirroring the
/// `percent_decode_str` helper in `pacquet-network`'s proxy module.
fn percent_decode_str(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[idx + 1..idx + 3]).ok();
            if let Some(byte) = hex.and_then(|hex_digits| u8::from_str_radix(hex_digits, 16).ok()) {
                out.push(byte);
                idx += 3;
                continue;
            }
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests;
