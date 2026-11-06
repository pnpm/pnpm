//! The workspace's members and the source files each one owns.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// One workspace member: what it is called, where it is, and which files
/// are the roots of its compilation units.
#[derive(Debug)]
pub struct Member {
    /// The package name, as the exemption list spells it.
    pub name: String,
    /// The directory holding its manifest.
    pub directory: PathBuf,
    /// Whether its manifest takes the workspace lint table, which is what
    /// puts the `unsafe_code` lint in front of its compilation.
    pub inherits_workspace_lints: bool,
    /// The crate roots: the library, each binary, and each integration
    /// test, example and benchmark. Each is a separate crate, so an inner
    /// attribute on one does not reach the others.
    pub roots: Vec<PathBuf>,
    /// Every `.rs` file under the member, roots included.
    pub sources: Vec<PathBuf>,
}

/// Whether the manifest at `path` declares a workspace, which is what
/// marks the repository root.
pub(crate) fn declares_a_workspace(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<WorkspaceManifest>(&text).ok())
        .is_some_and(|manifest| manifest.workspace.is_some())
}

/// The level the workspace manifest sets `unsafe_code` to, if it sets one.
///
/// Cargo takes both the bare `"warn"` and the `{ level = "warn" }` form, so
/// a manifest written either way reads the same here.
pub(crate) fn workspace_unsafe_code_level(root: &Path) -> Option<String> {
    let manifest: WorkspaceManifest = read_manifest(&root.join("Cargo.toml"));
    match manifest.workspace?.lints.rust.get("unsafe_code")? {
        toml::Value::String(level) => Some(level.clone()),
        toml::Value::Table(table) => table
            .get("level")?
            .as_str()
            .map(ToOwned::to_owned),
        _ => None,
    }
}

/// Every member of the workspace rooted at `root`.
///
/// The member globs are read from the workspace manifest rather than
/// hard-coded, so a crate added under one of them is audited without this
/// crate being touched.
#[must_use]
pub fn members(root: &Path) -> Vec<Member> {
    let manifest: WorkspaceManifest = read_manifest(&root.join("Cargo.toml"));
    let workspace = manifest.workspace.expect("the root manifest has a workspace table");
    let excluded: Vec<PathBuf> = workspace.exclude
        .iter()
        .map(|path| root.join(path))
        .collect();
    workspace.members
        .iter()
        .flat_map(|pattern| member_directories(root, pattern))
        .filter(|directory| !excluded.contains(directory))
        .map(|directory| member(&directory))
        .collect()
}

/// The directories one member pattern names. Only the trailing-`*` form
/// the workspace uses is expanded; anything else is a literal path.
fn member_directories(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let Some(parent) = pattern.strip_suffix("/*") else {
        return vec![root.join(pattern)];
    };
    let mut directories: Vec<PathBuf> = std::fs::read_dir(root.join(parent))
        .unwrap_or_else(|error| panic!("read {parent}: {error}"))
        .map(|entry| entry.expect("read a workspace member directory").path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    directories.sort();
    directories
}

fn member(directory: &Path) -> Member {
    let manifest: PackageManifest = read_manifest(&directory.join("Cargo.toml"));
    let roots = roots(directory, &manifest);
    let inherits_workspace_lints = manifest.lints.is_some_and(|lints| lints.workspace);
    let package =
        manifest.package.unwrap_or_else(|| panic!("{} has a package table", directory.display()));
    Member {
        name: package.name,
        directory: directory.to_path_buf(),
        inherits_workspace_lints,
        roots,
        sources: sources(directory),
    }
}

/// The crate roots of one member: the paths its manifest names, plus the
/// ones Cargo discovers by convention.
fn roots(directory: &Path, manifest: &PackageManifest) -> Vec<PathBuf> {
    let named = manifest.lib
        .iter()
        .chain(&manifest.bin)
        .chain(&manifest.test)
        .chain(&manifest.bench)
        .chain(&manifest.example)
        .filter_map(|target| target.path.as_deref())
        .map(|path| directory.join(path));
    let conventional = ["src/lib.rs", "src/main.rs"]
        .into_iter()
        .map(|path| directory.join(path))
        .chain(discovered(directory, "src/bin"))
        .chain(discovered(directory, "tests"))
        .chain(discovered(directory, "benches"))
        .chain(discovered(directory, "examples"));
    let mut roots: Vec<PathBuf> = named
        .chain(conventional)
        .filter(|path| path.is_file())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// The crate roots Cargo discovers in `subdirectory`: each `.rs` file
/// directly in it, and each `main.rs` one level down.
fn discovered(directory: &Path, subdirectory: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory.join(subdirectory)) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.expect("read a target directory").path())
        .filter_map(|path| {
            if path
                .extension()
                .is_some_and(|extension| extension == "rs")
            {
                return Some(path);
            }
            let nested = path.join("main.rs");
            nested.is_file().then_some(nested)
        })
        .collect()
}

/// Every `.rs` file under `directory`, skipping anything Cargo generated.
fn sources(directory: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = read_entries(&current);
        pending.extend(entries.directories);
        sources.extend(entries.sources);
    }
    sources.sort();
    sources
}

/// What one directory holds that the walk cares about.
struct Entries {
    /// The subdirectories to walk into.
    directories: Vec<PathBuf>,
    /// The `.rs` files directly in it.
    sources: Vec<PathBuf>,
}

/// Read one directory, keeping its subdirectories and its `.rs` files.
///
/// A nested `target` is a build directory rather than source, so it is
/// left out. A directory that cannot be read is a failure rather than an
/// empty answer: this walk decides which sources the policy is checked
/// against, so one it silently skipped would pass whatever it holds.
///
/// Each entry is classified by its own type rather than by following its
/// path, so a symlink is never traversed: one pointing at an ancestor
/// would otherwise keep the walk going forever, and one pointing at a
/// sibling would scan it twice.
fn read_entries(directory: &Path) -> Entries {
    let entries = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
    let mut found = Entries { directories: Vec::new(), sources: Vec::new() };
    for entry in entries {
        let entry = entry.expect("read a source directory");
        let file_type = entry.file_type().expect("read a source entry's type");
        let path = entry.path();
        if file_type.is_dir() {
            if path
                .file_name()
                .is_none_or(|name| name != "target")
            {
                found.directories.push(path);
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            found.sources.push(path);
        }
    }
    found
}

fn read_manifest<Manifest: serde::de::DeserializeOwned>(path: &Path) -> Manifest {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    toml::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

#[derive(Deserialize)]
struct WorkspaceManifest {
    workspace: Option<Workspace>,
}

#[derive(Deserialize)]
struct Workspace {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    lints: WorkspaceLints,
}

#[derive(Default, Deserialize)]
struct WorkspaceLints {
    #[serde(default)]
    rust: std::collections::BTreeMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct Lints {
    #[serde(default)]
    workspace: bool,
}

#[derive(Deserialize)]
struct PackageManifest {
    package: Option<Package>,
    lints: Option<Lints>,
    lib: Option<Target>,
    #[serde(default)]
    bin: Vec<Target>,
    #[serde(default)]
    test: Vec<Target>,
    #[serde(default)]
    bench: Vec<Target>,
    #[serde(default)]
    example: Vec<Target>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
}

#[derive(Deserialize)]
struct Target {
    path: Option<String>,
}

#[cfg(test)]
mod tests;
