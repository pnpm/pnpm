//! The `bin` field, with `directories.bin` expanded into it.

use crate::{declared_bin_entries, normalize_package_relative_path};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The commands the manifest declares, as an object of command name to
/// package-relative path, or `None` when it declares none.
///
/// A string `bin` names one command after the package, without its scope. A
/// command name is its last path segment, and a path is normalized to `/`
/// separators without `./` segments, as npm's `binRefs` step writes both. With
/// no `bin`, every file under `directories.bin` that does not start with `.`
/// becomes a command named after the file, as npm's `binDir` step expands it.
pub(super) fn derive_bin(manifest: &Value, files: &BTreeSet<String>) -> Option<Value> {
    let declared = manifest
        .get("bin")
        .filter(|bin| !bin.is_null() && bin.as_str() != Some(""));
    let entries = match declared {
        Some(bin) => {
            let name = manifest
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim);
            declared_bin_entries(bin, name)
        }
        None => directory_bin_entries(manifest, files),
    };
    let commands: std::collections::BTreeMap<String, Value> = entries
        .into_iter()
        .filter_map(|(command, path)| normalize_entry(&command, &path))
        .collect();
    (!commands.is_empty()).then(|| Value::Object(commands.into_iter().collect::<Map<_, _>>()))
}

fn normalize_entry(command: &str, path: &str) -> Option<(String, Value)> {
    let command = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .trim();
    let path = normalize_package_relative_path(path).unwrap_or_else(|| path.trim().to_string());
    let usable = !command.is_empty() && !path.is_empty();
    usable.then(|| (command.to_string(), Value::String(path)))
}

fn directory_bin_entries(manifest: &Value, files: &BTreeSet<String>) -> Vec<(String, String)> {
    let Some(raw_dir) = manifest.pointer("/directories/bin").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(dir) = normalize_package_relative_path(raw_dir) else { return Vec::new() };
    files
        .iter()
        .filter_map(|file| {
            let inside = if dir.is_empty() { Some(file.as_str()) } else { inside_dir(file, &dir) };
            inside.filter(|relative| !relative.starts_with('.'))?;
            let command = file.rsplit('/').next().unwrap_or(file);
            Some((command.to_string(), file.clone()))
        })
        .collect()
}

fn inside_dir<'file>(file: &'file str, dir: &str) -> Option<&'file str> {
    file.strip_prefix(dir)?.strip_prefix('/')
}
