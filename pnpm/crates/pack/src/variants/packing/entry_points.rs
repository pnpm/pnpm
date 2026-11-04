//! The entry points an archive's manifest names must be files the archive
//! holds.

use super::{BTreeSet, IndexMap, PackError, PathBuf, Value, VariantEntry, required_field_paths};

/// Refuse an archive whose manifest's `main`, `browser`, or `bin` names a
/// file the archive lacks. A `bin` names its file exactly. A `main` or
/// `browser` names a file as Node.js resolves it (see [`resolves_module`]).
pub(super) fn check_entry_points(
    entry: &VariantEntry,
    manifest: &Value,
    (visible, files_map): (&BTreeSet<String>, &IndexMap<String, PathBuf>),
) -> Result<(), PackError> {
    let packed = |path: &str| visible.contains(&format!("package/{path}"));
    let missing =
        |path: String| PackError::VariantEntryPointMissing { targets: entry.label(), path };
    let (bins, modules) = entry_point_fields(manifest);
    if let Some(bin) = required_field_paths(&bins)
        .into_iter()
        .find(|path| !packed(path))
    {
        return Err(missing(bin));
    }
    match required_field_paths(&modules)
        .into_iter()
        .find(|path| !resolves_module(path, (visible, files_map)))
    {
        Some(module) => Err(missing(module)),
        None => Ok(()),
    }
}

/// Whether the module `path` names resolves, as Node.js resolves it, to a
/// file of `files_map`: the file, the file with an extension, or the
/// directory's own `main`, then its `index`.
fn resolves_module(
    path: &str,
    (visible, files_map): (&BTreeSet<String>, &IndexMap<String, PathBuf>),
) -> bool {
    let packed = |path: &str| visible.contains(&format!("package/{path}"));
    let as_file = |path: &str| {
        packed(path)
            || NODE_EXTENSIONS
                .iter()
                .any(|extension| packed(&format!("{path}{extension}")))
    };
    let as_index = |path: &str| {
        NODE_EXTENSIONS
            .iter()
            .any(|extension| packed(&format!("{path}/index{extension}")))
    };
    as_file(path)
        || directory_main(path, files_map).is_some_and(|main| as_file(&main) || as_index(&main))
        || as_index(path)
}

/// The package path the `main` of the packed `package.json` in the
/// directory `dir` names, if there is one inside the package.
fn directory_main(dir: &str, files_map: &IndexMap<String, PathBuf>) -> Option<String> {
    let source = files_map.get(&format!("package/{dir}/package.json"))?;
    let manifest: Value = serde_json::from_slice(&std::fs::read(source).ok()?).ok()?;
    let main = manifest.get("main")?.as_str()?;
    package_path(&format!("{dir}/{main}"))
}

/// `path` with its `.` and `..` segments resolved, or `None` when it leaves
/// the package.
fn package_path(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    Some(segments.join("/"))
}

/// The extensions Node.js tries on a module path.
const NODE_EXTENSIONS: [&str; 3] = [".js", ".json", ".node"];

/// `manifest`'s `bin` alone, and its `main` and `browser` alone.
fn entry_point_fields(manifest: &Value) -> (Value, Value) {
    let only = |fields: &[&str]| {
        let kept = fields
            .iter()
            .filter_map(|field| Some(((*field).to_string(), manifest.get(*field)?.clone())))
            .collect();
        Value::Object(kept)
    };
    (only(&["bin"]), only(&["main", "browser"]))
}
