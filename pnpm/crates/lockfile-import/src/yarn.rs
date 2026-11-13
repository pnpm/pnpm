//! Version extraction from yarn's two lockfile formats.

use derive_more::{Display, Error};
use pnpm_resolving_resolver_base::is_valid_semver_range;

use crate::{VersionsByPackageName, add_version, split_descriptor};

const METADATA_KEY: &str = "__metadata";
const BERRY_METADATA_PREFIXES: [&str; 3] = ["__metadata:", r#""__metadata":"#, "'__metadata':"];
const QUOTES: [char; 2] = ['"', '\''];

#[derive(Clone, Copy)]
enum YarnDialect {
    Classic,
    Berry,
}

/// A `yarn.lock` whose syntax belongs to neither yarn dialect.
///
/// Only syntax is rejected. An entry that parses but records no version
/// is left to the caller, which treats a missing pin as one range to
/// resolve rather than as a failure.
#[derive(Debug, Display, Error)]
pub enum YarnSyntaxError {
    #[display("line {line} is not an entry key: a yarn.lock entry key ends with \":\"")]
    EntryKeyExpected { line: usize },

    #[display("line {line} is indented but does not belong to an entry")]
    OrphanedProperty { line: usize },

    #[display("line {line} is neither a property with a value nor the start of a nested block")]
    PropertyExpected { line: usize },

    #[display("the top level of a yarn berry lockfile must be a mapping of entries")]
    BerryRootNotAMapping,

    #[display("entry {entry:?} of a yarn berry lockfile is not a mapping")]
    BerryEntryNotAMapping {
        #[error(not(source))]
        entry: String,
    },

    #[display("yarn berry lockfile is not valid YAML")]
    Yaml { source: yaml_serde::Error },
}

/// Collect every version a yarn lockfile pins.
///
/// Both dialects list one entry per resolved package, keyed by the
/// descriptors that resolved to it and carrying the chosen version as a
/// `version` property. Yarn berry writes YAML and is parsed as such;
/// yarn classic's bespoke format gets the line parser below.
///
/// A lockfile holding no entries at all is valid and yields nothing:
/// yarn writes a header-only `yarn.lock` for a project with no
/// dependencies.
pub fn collect_yarn_lockfile_versions(
    contents: &str,
    versions: &mut VersionsByPackageName,
) -> Result<(), YarnSyntaxError> {
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
    if is_berry(contents) {
        collect_berry_versions(contents, versions)
    } else {
        collect_classic_versions(contents, versions)
    }
}

/// Yarn berry stamps every lockfile it writes with a top-level `__metadata:`
/// YAML mapping. Checking for that unindented key avoids misclassifying yarn
/// classic lockfiles that name a `__metadata*` package.
fn is_berry(contents: &str) -> bool {
    contents.lines().any(is_berry_metadata_line)
}

fn is_berry_metadata_line(line: &str) -> bool {
    BERRY_METADATA_PREFIXES
        .into_iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .is_some_and(|rest| {
            let rest = rest.trim_start();
            rest.is_empty() || rest.starts_with('#') || rest.starts_with('{')
        })
}

fn collect_berry_versions(
    contents: &str,
    versions: &mut VersionsByPackageName,
) -> Result<(), YarnSyntaxError> {
    let document: yaml_serde::Value =
        yaml_serde::from_str(contents).map_err(|source| YarnSyntaxError::Yaml { source })?;
    let entries = document.as_mapping().ok_or(YarnSyntaxError::BerryRootNotAMapping)?;

    for (key, entry) in entries {
        let Some(key) = key.as_str() else {
            return Err(YarnSyntaxError::BerryRootNotAMapping);
        };
        if key == METADATA_KEY {
            continue;
        }
        let entry = entry
            .as_mapping()
            .ok_or_else(|| YarnSyntaxError::BerryEntryNotAMapping { entry: key.to_string() })?;
        let Some(version) = entry.get("version").and_then(scalar_as_str) else {
            continue;
        };
        record_berry_entry_versions(key, entry, &version, versions);
    }

    Ok(())
}

fn record_berry_entry_versions(
    key: &str,
    entry: &yaml_serde::Mapping,
    version: &str,
    versions: &mut VersionsByPackageName,
) {
    if let Some(name) = entry
        .get("resolution")
        .and_then(yaml_serde::Value::as_str)
        .and_then(berry_resolution_package_name)
    {
        add_version(versions, name, version);
        return;
    }
    for name in descriptor_package_names(key, YarnDialect::Berry) {
        add_version(versions, name, version);
    }
}

fn berry_resolution_package_name(resolution: &str) -> Option<&str> {
    let (name, spec) = split_descriptor(resolution.trim().trim_matches(QUOTES))?;
    (!name.is_empty() && spec.starts_with("npm:")).then_some(name)
}

/// A berry `version:` is usually an unquoted scalar, which YAML types as
/// a number when it looks like one (`version: 3`).
fn scalar_as_str(value: &yaml_serde::Value) -> Option<String> {
    match value {
        yaml_serde::Value::String(version) => Some(version.clone()),
        yaml_serde::Value::Number(version) => Some(version.to_string()),
        _ => None,
    }
}

fn collect_classic_versions(
    contents: &str,
    versions: &mut VersionsByPackageName,
) -> Result<(), YarnSyntaxError> {
    let mut entry_names: Vec<&str> = Vec::new();
    let mut property_indent: Option<usize> = None;

    for (offset, line) in contents.lines().enumerate() {
        let number = offset + 1;
        let content = line.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = line.len() - content.len();

        if indent == 0 {
            entry_names.clear();
            property_indent = None;
            start_classic_entry(content, number, &mut entry_names)?;
            continue;
        }

        let Some(version) =
            entry_version(content, number, indent, &entry_names, &mut property_indent)?
        else {
            continue;
        };
        for name in &entry_names {
            add_version(versions, name, version);
        }
    }

    Ok(())
}

/// The version one property line of a classic entry declares, if it is that
/// line.
///
/// A `dependencies` block nests one level deeper and lists names, one of which
/// may itself be `version`, so only lines at the entry's own property indent
/// are read.
fn entry_version<'a>(
    content: &'a str,
    line: usize,
    indent: usize,
    entry_names: &[&str],
    property_indent: &mut Option<usize>,
) -> Result<Option<&'a str>, YarnSyntaxError> {
    if entry_names.is_empty() {
        return Err(YarnSyntaxError::OrphanedProperty { line });
    }
    let property =
        ClassicProperty::parse(content).ok_or(YarnSyntaxError::PropertyExpected { line })?;
    if indent != *property_indent.get_or_insert(indent) {
        return Ok(None);
    }
    match property {
        ClassicProperty::Valued { key: "version", value } => Ok(Some(value)),
        _ => Ok(None),
    }
}

/// Read the descriptors one top-level entry key declares.
fn start_classic_entry<'a>(
    content: &'a str,
    line: usize,
    entry_names: &mut Vec<&'a str>,
) -> Result<(), YarnSyntaxError> {
    let key = content
        .trim_end()
        .strip_suffix(':')
        .ok_or(YarnSyntaxError::EntryKeyExpected { line })?;
    entry_names.extend(descriptor_package_names(key, YarnDialect::Classic));
    Ok(())
}

/// A property line of a yarn classic entry, which either carries a value
/// (`version "1.2.3"`) or opens a nested block (`dependencies:`).
enum ClassicProperty<'a> {
    Valued { key: &'a str, value: &'a str },
    NestedBlock,
}

impl<'a> ClassicProperty<'a> {
    fn parse(content: &'a str) -> Option<Self> {
        if content.ends_with(':') {
            return Some(Self::NestedBlock);
        }
        let (key, value) = split_key_and_value(content)?;
        Some(Self::Valued { key, value })
    }
}

/// Split `key "value"` on the whitespace after the key, honoring a
/// quoted key that may itself contain whitespace.
fn split_key_and_value(content: &str) -> Option<(&str, &str)> {
    let key_end = if let Some(quote) = content.strip_prefix(QUOTES) {
        let closing = quote.find(QUOTES)?;
        content.len() - quote.len() + closing + 1
    } else {
        content.find(char::is_whitespace)?
    };
    let (key, value) = content.split_at(key_end);
    let value = value.trim();
    (!value.is_empty()).then(|| (key.trim().trim_matches(QUOTES), value.trim_matches(QUOTES)))
}

/// An entry's key lists every descriptor that resolved to it, comma separated.
/// Splitting at the first `@` after the leading character preserves scopes and
/// extracts the target package name when an `npm:` alias redirects to another
/// package (`my-alias@npm:minimatch@^3.0.4`).
fn descriptor_package_names(key: &str, dialect: YarnDialect) -> impl Iterator<Item = &str> {
    key.split(',').filter_map(move |descriptor| descriptor_package_name(descriptor, dialect))
}

fn descriptor_package_name(descriptor: &str, dialect: YarnDialect) -> Option<&str> {
    let (name, spec) = split_descriptor(descriptor.trim().trim_matches(QUOTES))?;
    if name.is_empty() {
        return None;
    }
    let Some(npm_spec) = spec.strip_prefix("npm:") else {
        return Some(name);
    };
    if let Some((target, _)) = split_descriptor(npm_spec) {
        return (!target.is_empty()).then_some(target);
    }
    if npm_spec.starts_with('@') && npm_spec.contains('/') {
        return Some(npm_spec);
    }
    match dialect {
        YarnDialect::Classic if !npm_spec.is_empty() && !is_valid_semver_range(npm_spec) => {
            Some(npm_spec)
        }
        YarnDialect::Classic | YarnDialect::Berry => Some(name),
    }
}

#[cfg(test)]
mod tests;
