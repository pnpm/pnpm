//! Version extraction from yarn's two lockfile formats.

use derive_more::{Display, Error};

use crate::{VersionsByPackageName, add_version};

const METADATA_KEY: &str = "__metadata";
const QUOTES: [char; 2] = ['"', '\''];

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
    if is_berry(contents) {
        collect_berry_versions(contents, versions)
    } else {
        collect_classic_versions(contents, versions)
    }
}

/// Yarn berry stamps every lockfile it writes with a `__metadata` block.
/// The TypeScript CLI looks for the same marker anywhere in the file.
fn is_berry(contents: &str) -> bool {
    contents.lines().any(|line| line.trim_start().starts_with(METADATA_KEY))
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
        for name in descriptor_package_names(key) {
            add_version(versions, name, &version);
        }
    }

    Ok(())
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
            let key = content
                .strip_suffix(':')
                .ok_or(YarnSyntaxError::EntryKeyExpected { line: number })?;
            if key != METADATA_KEY {
                entry_names.extend(descriptor_package_names(key));
            }
            continue;
        }

        if entry_names.is_empty() {
            return Err(YarnSyntaxError::OrphanedProperty { line: number });
        }
        let property = ClassicProperty::parse(content)
            .ok_or(YarnSyntaxError::PropertyExpected { line: number })?;
        // A `dependencies` block nests one level deeper and lists names,
        // one of which may itself be `version`.
        if indent != *property_indent.get_or_insert(indent) {
            continue;
        }
        if let ClassicProperty::Valued { key: "version", value } = property {
            for name in &entry_names {
                add_version(versions, name, value);
            }
        }
    }

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

/// An entry's key lists every descriptor that resolved to it, comma
/// separated. A descriptor's package name is what precedes its last
/// `@`, which keeps a scope's leading `@` and drops yarn berry's
/// protocol along with the range (`minimatch@npm:^3.0.4`).
fn descriptor_package_names(key: &str) -> impl Iterator<Item = &str> {
    key.split(',').filter_map(|descriptor| {
        let descriptor = descriptor.trim().trim_matches(QUOTES);
        let name = &descriptor[..descriptor.rfind('@')?];
        (!name.is_empty()).then_some(name)
    })
}

#[cfg(test)]
mod tests;
