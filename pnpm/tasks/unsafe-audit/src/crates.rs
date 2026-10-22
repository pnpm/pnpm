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
    let package =
        manifest.package.unwrap_or_else(|| panic!("{} has a package table", directory.display()));
    Member {
        name: package.name,
        directory: directory.to_path_buf(),
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
/// left out; an unreadable directory contributes nothing.
fn read_entries(directory: &Path) -> Entries {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Entries { directories: Vec::new(), sources: Vec::new() };
    };
    let (directories, files): (Vec<PathBuf>, Vec<PathBuf>) = entries
        .map(|entry| entry.expect("read a source directory").path())
        .partition(|path| path.is_dir());
    Entries {
        directories: directories
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .is_none_or(|name| name != "target")
            })
            .collect(),
        sources: files
            .into_iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "rs")
            })
            .collect(),
    }
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
}

#[derive(Deserialize)]
struct PackageManifest {
    package: Option<Package>,
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
