//! Version extraction from yarn's two lockfile formats.

use crate::{VersionsByPackageName, add_version};

const METADATA_KEY: &str = "__metadata";
const QUOTES: [char; 2] = ['"', '\''];

/// Collect every version a yarn lockfile pins.
///
/// Both yarn formats list one entry per resolved package, keyed by the
/// descriptors that resolved to it and carrying the chosen version as a
/// `version` property. Yarn classic writes `version "1.2.3"`; yarn berry
/// writes YAML, `version: 1.2.3`. Only that one property is needed, so a
/// single line scanner serves both formats.
pub fn collect_yarn_lockfile_versions(contents: &str, versions: &mut VersionsByPackageName) {
    let mut entry_names: Vec<&str> = Vec::new();
    let mut property_indent: Option<usize> = None;

    for line in contents.lines() {
        let content = line.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = line.len() - content.len();

        if indent == 0 {
            entry_names.clear();
            property_indent = None;
            if let Some(key) = content.strip_suffix(':')
                && key != METADATA_KEY
            {
                entry_names.extend(descriptor_package_names(key));
            }
            continue;
        }

        if entry_names.is_empty() {
            continue;
        }
        // An entry's `dependencies` block is indented one level deeper
        // and lists names, one of which may itself be `version`.
        if indent != *property_indent.get_or_insert(indent) {
            continue;
        }
        if let Some(version) = version_property_value(content) {
            for name in &entry_names {
                add_version(versions, name, version);
            }
        }
    }
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

fn version_property_value(property: &str) -> Option<&str> {
    let separated = property.strip_prefix("version")?;
    let value = match separated.strip_prefix(':') {
        Some(value) => value,
        None if separated.starts_with(char::is_whitespace) => separated,
        None => return None,
    };
    Some(value.trim().trim_matches(QUOTES))
}

#[cfg(test)]
mod tests;
