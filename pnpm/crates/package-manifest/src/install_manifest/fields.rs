//! Normalizations of the identity, dependency, platform, and lifecycle-script
//! fields.

use super::NODE_GYP_REBUILD;
use crate::{is_truthy, manifest_opts_out_of_gyp_build};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The scripts an install runs, in the order npm runs them.
const LIFECYCLE_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

/// `name` trimmed, as npm's `normalizeData` step trims it, and `version`
/// cleaned the way npm cleans it, then compared without build metadata.
pub(super) fn insert_identity(fields: &mut Map<String, Value>, manifest: &Value) {
    if let Some(name) = manifest.get("name").and_then(Value::as_str) {
        fields.insert("name".to_string(), Value::String(name.trim().to_string()));
    }
    if let Some(version) = manifest.get("version").and_then(Value::as_str) {
        fields.insert("version".to_string(), Value::String(clean_version(version)));
    }
}

/// A version as `semver.clean` writes it, without build metadata: surrounding
/// whitespace and a leading `=` or `v` dropped, and everything from `+` on.
pub(super) fn clean_version(raw: &str) -> String {
    let trimmed = raw.trim();
    let unprefixed = trimmed
        .strip_prefix('=')
        .unwrap_or(trimmed)
        .trim_start();
    let unprefixed = unprefixed
        .strip_prefix(['v', 'V'])
        .unwrap_or(unprefixed);
    let without_build = unprefixed.split_once('+').map_or(unprefixed, |(version, _)| version);
    without_build.to_string()
}

/// The dependency maps and `bundleDependencies`.
///
/// A name that `dependencies` lists with the same specifier as
/// `optionalDependencies` is optional: npm 9 and earlier copied every optional
/// dependency into `dependencies` before publishing. `bundleDependencies: true`
/// means every name in `dependencies`, as npm expands it.
pub(super) fn insert_dependencies(fields: &mut Map<String, Value>, manifest: &Value) {
    let mut dependencies = dependency_map(manifest, "dependencies");
    let optional = dependency_map(manifest, "optionalDependencies");
    let bundled = bundled_names(manifest);
    dependencies.retain(|name, spec| optional.get(name) != Some(spec));
    insert_non_empty(fields, "dependencies", dependencies);
    insert_non_empty(fields, "optionalDependencies", optional);
    insert_non_empty(fields, "peerDependencies", dependency_map(manifest, "peerDependencies"));
    let peer_meta = manifest
        .get("peerDependenciesMeta")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    insert_non_empty(fields, "peerDependenciesMeta", peer_meta);
    if !bundled.is_empty() {
        let names = bundled
            .into_iter()
            .map(Value::String)
            .collect();
        fields.insert("bundleDependencies".to_string(), Value::Array(names));
    }
}

fn dependency_map(manifest: &Value, field: &str) -> Map<String, Value> {
    let Some(map) = manifest.get(field).and_then(Value::as_object) else {
        return Map::new();
    };
    map.iter()
        .map(|(name, spec)| (name.clone(), normalize_spec(spec)))
        .collect()
}

/// A hosted-git shorthand (`user/repo`, optionally with a `#ref`) as the
/// `github:` specifier npm expands it to. Every other specifier is kept.
fn normalize_spec(spec: &Value) -> Value {
    match spec.as_str() {
        Some(raw) if is_github_shorthand(raw) => Value::String(format!("github:{raw}")),
        _ => spec.clone(),
    }
}

fn is_github_shorthand(spec: &str) -> bool {
    let repository = spec.split_once('#').map_or(spec, |(repository, _)| repository);
    let Some((user, repo)) = repository.split_once('/') else { return false };
    is_shorthand_segment(user) && is_shorthand_segment(repo) && !user.starts_with(['.', '@'])
}

fn is_shorthand_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The names `bundleDependencies` (or its `bundledDependencies` alias) lists.
fn bundled_names(manifest: &Value) -> BTreeSet<String> {
    let declared = manifest
        .get("bundleDependencies")
        .or_else(|| manifest.get("bundledDependencies"));
    match declared {
        Some(Value::Bool(true)) => manifest
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|dependencies| dependencies.keys().cloned().collect())
            .unwrap_or_default(),
        Some(Value::Array(names)) => string_items(names),
        _ => BTreeSet::new(),
    }
}

/// `os`, `cpu`, `libc`, and `engines`. A platform list is a set; a single
/// string is a list of one. `engines.node: "*"` admits every Node.js, which
/// is what an absent `engines.node` says.
pub(super) fn insert_platform(fields: &mut Map<String, Value>, manifest: &Value) {
    for field in ["os", "cpu", "libc"] {
        let values = match manifest.get(field) {
            Some(Value::String(value)) => BTreeSet::from([value.clone()]),
            Some(Value::Array(values)) => string_items(values),
            _ => BTreeSet::new(),
        };
        if !values.is_empty() {
            let values = values
                .into_iter()
                .map(Value::String)
                .collect();
            fields.insert(field.to_string(), Value::Array(values));
        }
    }
    let mut engines = manifest
        .get("engines")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if engines.get("node").and_then(Value::as_str) == Some("*") {
        engines.remove("node");
    }
    insert_non_empty(fields, "engines", engines);
}

fn string_items(values: &[Value]) -> BTreeSet<String> {
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn insert_non_empty(fields: &mut Map<String, Value>, field: &str, map: Map<String, Value>) {
    if !map.is_empty() {
        let sorted = map.into_iter().collect::<std::collections::BTreeMap<_, _>>();
        fields.insert(field.to_string(), Value::Object(sorted.into_iter().collect()));
    }
}

/// The lifecycle scripts an install runs: the ones the manifest declares with
/// a value, and the `node-gyp rebuild` an installer runs for a root
/// `binding.gyp` when the manifest declares neither `install` nor `preinstall`
/// and does not set `gypfile: false`.
pub(super) fn lifecycle_scripts(manifest: &Value, binding_gyp: bool) -> Map<String, Value> {
    let declared = manifest.get("scripts").and_then(Value::as_object);
    let mut scripts = Map::new();
    for &name in LIFECYCLE_SCRIPTS {
        if let Some(script) = declared
            .and_then(|declared| declared.get(name))
            .filter(|script| is_truthy(script))
        {
            scripts.insert(name.to_string(), script.clone());
        }
    }
    let builds_binding = binding_gyp
        && !scripts.contains_key("install")
        && !scripts.contains_key("preinstall")
        && !manifest_opts_out_of_gyp_build(manifest);
    if builds_binding {
        scripts.insert("install".to_string(), Value::String(NODE_GYP_REBUILD.to_string()));
    }
    scripts
}

/// Replace the lifecycle scripts of `version` with `derived`, keeping every
/// other script it declares. A `scripts` left empty is removed.
pub(super) fn write_lifecycle_scripts(version: &mut Map<String, Value>, derived: Option<&Value>) {
    let mut scripts = version
        .get("scripts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for name in LIFECYCLE_SCRIPTS {
        scripts.remove(*name);
    }
    if let Some(derived) = derived.and_then(Value::as_object) {
        scripts.extend(
            derived
                .iter()
                .map(|(name, script)| (name.clone(), script.clone())),
        );
    }
    if scripts.is_empty() {
        version.remove("scripts");
    } else {
        version.insert("scripts".to_string(), Value::Object(scripts));
    }
}
