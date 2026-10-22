//! Relative paths in a local source, which are measured from the directory
//! of its path as `include` writes it, re-expressed from the workspace root
//! so the merged settings read the way a root manifest does.

use pnpm_fs::lexical_normalize;
use pnpm_local_spec::LocalSpec;
use std::path::Path;
use yaml_serde::{Mapping, Value};

/// Settings whose value is one path.
const PATH_SETTINGS: &[&str] = &[
    "cacheDir",
    "globalBinDir",
    "globalDir",
    "globalPnpmfile",
    "globalVirtualStoreDir",
    "lockfileDir",
    "modulesDir",
    "npmrcAuthFile",
    "onlyBuiltDependenciesFile",
    "patchesDir",
    "stateDir",
    "storeDir",
    "virtualStoreDir",
];

/// Rewrite the relative paths of `settings`, measured from `from_dir`, to be
/// measured from `to_dir`: path settings, `packages` globs, `pnpmfile`,
/// `patchedDependencies`, a path-like `scriptShell`, and local specifiers in
/// catalogs and `overrides`.
pub(super) fn rebase_settings(settings: &mut Mapping, from_dir: &Path, to_dir: &Path) {
    if lexical_normalize(from_dir) == lexical_normalize(to_dir) {
        return;
    }
    for (key, value) in settings.iter_mut() {
        rebase_setting(key.as_str().unwrap_or_default(), value, from_dir, to_dir);
    }
}

/// Rewrite the relative paths in `value`, the value of the setting `name`,
/// from `from_dir` to `to_dir`.
pub fn rebase_setting(name: &str, value: &mut Value, from_dir: &Path, to_dir: &Path) {
    let path = |value: &str| rebase_path(value, from_dir, to_dir);
    let specifier =
        |entry: &mut Value| rewrite(entry, &|value| rebase_specifier(value, from_dir, to_dir));
    let override_specifier =
        |entry: &mut Value| rewrite(entry, &|value| rebase_override(value, from_dir, to_dir));
    match name {
        "packages" => each_string(value, &|glob| rebase_glob(glob, from_dir, to_dir)),
        "pnpmfile" => each_string(value, &path),
        "scriptShell" => rewrite(value, &|shell| rebase_script_shell(shell, from_dir, to_dir)),
        "patchedDependencies" => each_value(value, &|entry| rewrite(entry, &path)),
        "catalog" => each_value(value, &specifier),
        "overrides" => each_value(value, &override_specifier),
        "catalogs" => each_value(value, &|catalog| each_value(catalog, &specifier)),
        name if PATH_SETTINGS.contains(&name) => rewrite(value, &path),
        _ => {}
    }
}

/// A `scriptShell` is a path only when it contains a separator; a bare
/// command name is looked up on `PATH`.
fn rebase_script_shell(shell: &str, from_dir: &Path, to_dir: &Path) -> Option<String> {
    shell
        .contains(['/', '\\'])
        .then(|| rebase_path(shell, from_dir, to_dir))
        .flatten()
}

/// `value`, a path relative to `from_dir`, as a path relative to `to_dir`.
/// `None` when `value` does not move: an absolute path, one under the home
/// directory, or one that starts with an environment variable.
///
/// A leading `./` is kept, and the result uses forward slashes.
#[must_use]
pub fn rebase_path(value: &str, from_dir: &Path, to_dir: &Path) -> Option<String> {
    if !moves_with_its_file(value) {
        return None;
    }
    let target = lexical_normalize(&from_dir.join(value.replace('\\', "/")));
    let relative = pathdiff::diff_paths(target, lexical_normalize(to_dir))?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    Some(match relative.as_str() {
        "" => ".".to_string(),
        _ if value.starts_with("./") || value.starts_with(r".\") => {
            if relative.starts_with('.') {
                relative
            } else {
                format!("./{relative}")
            }
        }
        _ => relative,
    })
}

fn moves_with_its_file(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        None | Some('/' | '\\' | '~' | '$') => false,
        Some(first) if first.is_ascii_alphabetic() => chars.next() != Some(':'),
        Some(_) => true,
    }
}

fn rebase_glob(glob: &str, from_dir: &Path, to_dir: &Path) -> Option<String> {
    match glob.strip_prefix('!') {
        Some(negated) => rebase_path(negated, from_dir, to_dir).map(|path| format!("!{path}")),
        None => rebase_path(glob, from_dir, to_dir),
    }
}

/// `value`, a `file:`, `link:` or bare-path specifier relative to
/// `from_dir`, re-anchored to `to_dir` the way pnpm re-anchors a catalog
/// entry for the project that uses it. `None` for any other specifier.
#[must_use]
pub fn rebase_specifier(value: &str, from_dir: &Path, to_dir: &Path) -> Option<String> {
    LocalSpec::parse_filesystem(value, &lexical_normalize(from_dir))
        .map(|spec| spec.render(Some(&lexical_normalize(to_dir))))
}

/// [`rebase_specifier`] for an `overrides` value, which also moves a
/// `workspace:` path: pnpm measures one in an override from the workspace
/// root.
fn rebase_override(value: &str, from_dir: &Path, to_dir: &Path) -> Option<String> {
    rebase_specifier(value, from_dir, to_dir)
        .or_else(|| {
            LocalSpec::parse_workspace_path(value, &lexical_normalize(from_dir))
                .map(|spec| spec.render(Some(&lexical_normalize(to_dir))))
        })
}

fn rewrite(value: &mut Value, rebase: &dyn Fn(&str) -> Option<String>) {
    if let Some(rebased) = value.as_str().and_then(rebase) {
        *value = Value::String(rebased);
    }
}

fn each_string(value: &mut Value, rebase: &dyn Fn(&str) -> Option<String>) {
    match value {
        Value::Sequence(items) => items.iter_mut().for_each(|item| rewrite(item, rebase)),
        scalar => rewrite(scalar, rebase),
    }
}

fn each_value(value: &mut Value, visit: &dyn Fn(&mut Value)) {
    if let Value::Mapping(entries) = value {
        entries.iter_mut().for_each(|(_, entry)| visit(entry));
    }
}
