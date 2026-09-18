//! Parsing of [Package URLs](https://github.com/package-url/purl-spec), the
//! `pkg:<type>/<namespace>/<name>@<version>` notation that identifies a
//! package across ecosystems.

use super::Shown;
use miette::Result;
use percent_encoding::percent_decode_str;
use pipe_trait::Pipe;

/// The URL scheme every Package URL starts with.
const SCHEME: &str = "pkg";

/// The package types pnpm installs, which are the only ones a selector may
/// name. The specification registers many more.
#[derive(Debug, strum::Display, Clone, Copy, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub(super) enum PurlType {
    Npm,
    Cargo,
    Pypi,
}

/// The components of a Package URL that pnpm can map onto a dependency.
///
/// Qualifiers and subpaths are rejected by [`Purl::parse`] instead of being
/// stored, because pnpm has no dependency field to put them in.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Purl {
    pub(super) package_type: PurlType,
    pub(super) namespace: Option<String>,
    pub(super) name: String,
    pub(super) version: Option<String>,
}

/// The part of `specifier` a [`Purl`] is parsed from, or [`None`] when
/// `specifier` is not a Package URL.
///
/// The scheme matches case-insensitively because a URL scheme is
/// case-insensitive, and the slashes a `pkg://npm/lodash` spelling puts
/// after it are dropped, both as the specification requires.
pub(super) fn strip_scheme(specifier: &str) -> Option<&str> {
    let (scheme, body) = specifier.split_once(':')?;
    scheme
        .eq_ignore_ascii_case(SCHEME)
        .then(|| body.trim_start_matches('/'))
}

impl Purl {
    /// Parses the body [`strip_scheme`] returned. `source` is the specifier
    /// as it was written, which the error messages quote back.
    pub(super) fn parse(body: &str, source: Shown<'_>) -> Result<Self> {
        reject_unsupported_components(body, source)?;
        let (package_type, path) = body.split_once('/').ok_or_else(|| missing_name(source))?;
        let package_type = parse_type(package_type, source)?;
        let (namespace, last_segment) = split_namespace(path.trim_matches('/'));
        let (name, version) = split_version(last_segment, source)?;
        let name = decode(name, source)?;
        if name.is_empty() {
            return Err(missing_name(source));
        }
        if name.contains('/') {
            return Err(miette::miette!("{source} has an invalid purl name"));
        }
        Ok(Purl {
            package_type,
            namespace: namespace
                .map(|namespace| decode_namespace(namespace, source))
                .transpose()?,
            name,
            version: version.map(|version| decode(version, source)).transpose()?,
        })
    }
}

/// Qualifiers select a repository, an architecture, or a distribution file,
/// and a subpath selects a directory inside the package. Honoring either
/// silently would install something other than what was asked for.
fn reject_unsupported_components(body: &str, source: Shown<'_>) -> Result<()> {
    let source = source.without_qualifiers();
    if body.contains('?') {
        return Err(miette::miette!("{source} carries purl qualifiers, which pnpm cannot honor"));
    }
    if body.contains('#') {
        return Err(miette::miette!("{source} carries a purl subpath, which pnpm cannot honor"));
    }
    Ok(())
}

fn parse_type(package_type: &str, source: Shown<'_>) -> Result<PurlType> {
    let well_formed = package_type.starts_with(|ch: char| ch.is_ascii_alphabetic())
        && package_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
    if !well_formed {
        let package_type = Shown(package_type);
        return Err(miette::miette!("{source} has an invalid purl type `{package_type}`"));
    }
    let package_type = package_type.to_ascii_lowercase();
    package_type.parse::<PurlType>().map_err(|_| {
        miette::miette!(
            "{source} has purl type `{package_type}`, but pnpm can add only `npm`, `cargo`, and `pypi` packages"
        )
    })
}

/// Splits the path that follows the type into its namespace and the segment
/// that carries the name.
///
/// The version is taken from the last segment rather than from the whole
/// path so that a scope written with an unencoded `@`, as in
/// `pkg:npm/@babel/core`, is read as a namespace and not as a version.
fn split_namespace(path: &str) -> (Option<&str>, &str) {
    match path.rsplit_once('/') {
        Some((namespace, last_segment)) => (Some(namespace), last_segment),
        None => (None, path),
    }
}

fn split_version<'a>(segment: &'a str, source: Shown<'_>) -> Result<(&'a str, Option<&'a str>)> {
    let Some((name, version)) = segment.rsplit_once('@') else {
        return Ok((segment, None));
    };
    if version.is_empty() {
        return Err(miette::miette!("missing version after `@` in {source}"));
    }
    Ok((name, Some(version)))
}

fn decode_namespace(namespace: &str, source: Shown<'_>) -> Result<String> {
    let mut segments = Vec::new();
    for segment in namespace.split('/') {
        let segment = decode(segment, source)?;
        if segment.is_empty() || segment.contains('/') {
            return Err(miette::miette!("{source} has an invalid purl namespace"));
        }
        segments.push(segment);
    }
    Ok(segments.join("/"))
}

fn decode(component: &str, source: Shown<'_>) -> Result<String> {
    component
        .pipe(percent_decode_str)
        .decode_utf8()
        .map(std::borrow::Cow::into_owned)
        .map_err(|_| miette::miette!("{source} is not valid UTF-8 once percent-decoded"))
}

fn missing_name(source: Shown<'_>) -> miette::Report {
    miette::miette!("{source} is missing a purl name")
}

#[cfg(test)]
mod tests;
