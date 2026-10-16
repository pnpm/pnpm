//! The `include` index of `pnpm-workspace.yaml`: settings delegated to other
//! files and to config dependencies.
//!
//! The root manifest maps each source to the settings it may define. This
//! module reads the local sources, checks that every setting has one owner,
//! and merges the local sources into one document the settings readers parse
//! as if it were the root manifest. Config dependency sources are only
//! validated here; their values are read once the dependencies are installed.

pub use error::IncludeError;
pub use ownership::{Ownership, SettingOwner};
pub use package::{PackageEntry, config_dependency_dir};
pub use rebase::{rebase_path, rebase_setting, rebase_specifier};
pub use selector::{SettingSlot, canonical_setting_name};

use indexmap::IndexMap;
use same_file::Handle;
use std::{
    fmt, fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};
use yaml_serde::{Mapping, Value};

mod error;
mod merge;
mod normalize;
mod ownership;
mod package;
mod rebase;
mod selector;

/// The root-manifest key holding the include index.
pub const INCLUDE_KEY: &str = "include";

/// The delegation value that lets a local source define any setting.
const UNRESTRICTED: &str = "*";

/// Settings a config dependency can neither define nor be delegated: they
/// control configuration loading, select code, govern installing the
/// dependency itself, or hold paths that only mean something in one
/// repository.
pub const SETTINGS_FORBIDDEN_TO_PACKAGES: &[&str] = &[
    "bootConfig",
    "cacheDir",
    "configDependencies",
    "globalBinDir",
    "globalDir",
    "globalPnpmfile",
    "globalVirtualStoreDir",
    "lockfileDir",
    "modulesDir",
    "npmrcAuthFile",
    "onlyBuiltDependenciesFile",
    "packages",
    "patchedDependencies",
    "patchesDir",
    "pnpmfile",
    "scriptShell",
    "stateDir",
    "storeDir",
    "virtualStoreDir",
];

/// Where an included source lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncludeSource {
    /// A YAML file, by its spec as written (`./config/catalog.yaml`) and the
    /// path that spec resolves to.
    Local { spec: String, path: PathBuf },
    /// A declared config dependency, by name.
    Package(String),
}

impl fmt::Display for IncludeSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IncludeSource::Local { spec, .. } => formatter.write_str(spec),
            IncludeSource::Package(name) => formatter.write_str(name),
        }
    }
}

/// Which settings a source may define.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delegation {
    /// `'*'`: any setting no other source owns.
    Unrestricted,
    /// Exactly these settings, which the source owns even when it omits them.
    Listed(Vec<SettingSlot>),
}

/// One entry of the include index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    pub source: IncludeSource,
    pub delegation: Delegation,
}

/// The include index of a workspace, with its local sources loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceIncludes {
    /// In root-mapping order.
    pub includes: Vec<Include>,
    /// The text of each local source, keyed by its index in
    /// [`Self::includes`].
    pub fragments: IndexMap<usize, String>,
    /// The config entry of each config dependency source, keyed by its index
    /// in [`Self::includes`].
    pub package_entries: IndexMap<usize, PackageEntry>,
    pub ownership: Ownership,
}

impl WorkspaceIncludes {
    /// The source that owns `slot`, if any.
    #[must_use]
    pub fn owner(&self, slot: &SettingSlot) -> SettingOwner<'_> {
        self.ownership.owner(slot, &self.includes)
    }

    /// The config dependencies the index includes, with the settings each one
    /// is delegated.
    pub fn packages(&self) -> impl Iterator<Item = (&str, &[SettingSlot])> {
        self.includes.iter().filter_map(package_source)
    }

    /// The config dependencies the index includes whose config entry could not
    /// be read because they are not installed.
    pub fn unavailable_packages(&self) -> impl Iterator<Item = &str> {
        self.package_entries
            .iter()
            .filter_map(|(&position, entry)| match entry {
                PackageEntry::NotInstalled => {
                    package_source(&self.includes[position]).map(|(name, _)| name)
                }
                PackageEntry::Loaded { .. } => None,
            })
    }

    /// The local files the index includes.
    pub fn local_paths(&self) -> impl Iterator<Item = &Path> {
        self.includes
            .iter()
            .filter_map(|include| match &include.source {
                IncludeSource::Local { path, .. } => Some(path.as_path()),
                IncludeSource::Package(_) => None,
            })
    }
}

fn package_source(include: &Include) -> Option<(&str, &[SettingSlot])> {
    match (&include.source, &include.delegation) {
        (IncludeSource::Package(name), Delegation::Listed(slots)) => {
            Some((name.as_str(), slots.as_slice()))
        }
        _ => None,
    }
}

/// `pnpm-workspace.yaml` with its local includes merged in.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceManifestDocument {
    /// The root manifest.
    pub path: PathBuf,
    /// The merged settings: the root manifest's text as written when it has
    /// no include index.
    pub text: String,
    pub includes: Option<WorkspaceIncludes>,
}

/// Read `<dir>/pnpm-workspace.yaml` and the local files it includes.
///
/// Returns `Ok(None)` only when the root manifest does not exist.
pub fn read_workspace_manifest_document(
    dir: &Path,
) -> Result<Option<WorkspaceManifestDocument>, IncludeError> {
    let path = dir.join(crate::WORKSPACE_MANIFEST_FILENAME);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(IncludeError::ReadRoot { path, source }),
    };
    load_workspace_manifest_document(path, text).map(Some)
}

/// [`read_workspace_manifest_document`] for a root manifest already read.
pub fn load_workspace_manifest_document(
    path: PathBuf,
    text: String,
) -> Result<WorkspaceManifestDocument, IncludeError> {
    let Some(root) = root_mapping_with_include(&path, &text)? else {
        return Ok(WorkspaceManifestDocument { path, text, includes: None });
    };
    let (includes, merged) = load_includes(&path, root)?;
    Ok(WorkspaceManifestDocument { path, text: merged, includes: Some(includes) })
}

/// The root manifest's mapping when it has an include index. A manifest that
/// cannot hold one is left to the settings readers, which report it as they
/// always have.
fn root_mapping_with_include(path: &Path, text: &str) -> Result<Option<Mapping>, IncludeError> {
    // A key spelled with an escape sequence needs a backslash.
    if !text.contains(INCLUDE_KEY) && !text.contains('\\') {
        return Ok(None);
    }
    match parse_mapping(text) {
        Ok(root) => Ok(root.filter(|root| root.contains_key(INCLUDE_KEY))),
        // Strict where the settings readers are lenient: a document they read,
        // such as one with a duplicate key, must not have its include index
        // ignored here.
        Err(source) if settings_readers_see_include(text) => {
            Err(IncludeError::ParseRoot { path: path.to_path_buf(), source })
        }
        Err(_) => Ok(None),
    }
}

fn settings_readers_see_include(text: &str) -> bool {
    serde_saphyr::from_str::<IndexMap<String, serde::de::IgnoredAny>>(text)
        .is_ok_and(|root| root.contains_key(INCLUDE_KEY))
}

/// `text` as a mapping, or `None` for a document that is not one.
fn parse_mapping(text: &str) -> Result<Option<Mapping>, yaml_serde::Error> {
    let mapping = match normalize::normalize(yaml_serde::from_str::<Value>(text)?)? {
        Value::Mapping(mapping) => mapping,
        Value::Null => Mapping::new(),
        _ => return Ok(None),
    };
    normalize::check_catalogs(&mapping)?;
    Ok(Some(mapping))
}

fn load_includes(
    root_path: &Path,
    mut root: Mapping,
) -> Result<(WorkspaceIncludes, String), IncludeError> {
    let index = root.remove(INCLUDE_KEY).unwrap_or(Value::Null);
    let base_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
    let includes = selector::parse_index(&index, base_dir)?;
    let fragments = read_local_sources(root_path, &includes)?;
    let mut parsed = Vec::with_capacity(fragments.len());
    for (&position, text) in &fragments {
        parsed.push((position, parse_fragment(&includes[position].source, text, base_dir)?));
    }
    let ownership = Ownership::build(&root, &includes, &parsed)?;
    let merged = merge::merge(root, parsed.into_iter().map(|(_, fragment)| fragment));
    selector::check_package_sources(&includes, &merged)?;
    let (package_entries, package_settings) = read_package_sources(base_dir, &includes)?;
    let text = merge::render(merge::merge(merged, package_settings));
    Ok((WorkspaceIncludes { includes, fragments, package_entries, ownership }, text))
}

/// The config entry of each installed config dependency source, and the
/// settings of those entries.
fn read_package_sources(
    workspace_dir: &Path,
    includes: &[Include],
) -> Result<(IndexMap<usize, PackageEntry>, Vec<Mapping>), IncludeError> {
    let mut entries = IndexMap::new();
    let mut settings = Vec::new();
    for (position, include) in includes.iter().enumerate() {
        let Some((name, delegated)) = package_source(include) else {
            continue;
        };
        let entry = package::read_entry(workspace_dir, name)?;
        if let PackageEntry::Loaded { text, .. } = &entry {
            settings.push(package::check_entry(name, delegated, text)?);
        }
        entries.insert(position, entry);
    }
    Ok((entries, settings))
}

/// The text of each local source, rejecting a source that names the root
/// manifest or a file another source already names.
fn read_local_sources(
    root_path: &Path,
    includes: &[Include],
) -> Result<IndexMap<usize, String>, IncludeError> {
    let root_handle = Handle::from_path(root_path).ok();
    let mut seen: Vec<(Handle, &str)> = Vec::new();
    let mut fragments = IndexMap::new();
    for (position, include) in includes.iter().enumerate() {
        let IncludeSource::Local { spec, path } = &include.source else {
            continue;
        };
        let text = read_fragment(spec, path)?;
        let handle = Handle::from_path(path)
            .map_err(|source| IncludeError::ReadFragment { spec: spec.clone(), source })?;
        if root_handle.as_ref() == Some(&handle) {
            return Err(IncludeError::IncludesRoot { spec: spec.clone() });
        }
        if let Some((_, other)) = seen
            .iter()
            .find(|(seen, _)| *seen == handle)
        {
            return Err(IncludeError::DuplicateSource {
                spec: spec.clone(),
                other: (*other).to_string(),
            });
        }
        seen.push((handle, spec));
        fragments.insert(position, text);
    }
    Ok(fragments)
}

fn read_fragment(spec: &str, path: &Path) -> Result<String, IncludeError> {
    let read_error = |source| IncludeError::ReadFragment { spec: spec.to_string(), source };
    if !fs::metadata(path).map_err(read_error)?.is_file() {
        return Err(IncludeError::NotAFile { spec: spec.to_string() });
    }
    fs::read_to_string(path).map_err(read_error)
}

/// A local source's settings, rejecting a nested include index. Its relative
/// paths are re-expressed from `base_dir`, the workspace root.
fn parse_fragment(
    source: &IncludeSource,
    text: &str,
    base_dir: &Path,
) -> Result<Mapping, IncludeError> {
    let spec = source.to_string();
    let fragment = parse_mapping(text)
        .map_err(|source| IncludeError::ParseFragment { spec: spec.clone(), source })?
        .ok_or_else(|| IncludeError::FragmentNotAMapping { spec: spec.clone() })?;
    if fragment.contains_key(INCLUDE_KEY) {
        return Err(IncludeError::NestedInclude { spec });
    }
    let mut fragment = fragment;
    if let IncludeSource::Local { path, .. } = source {
        let fragment_dir = path.parent().unwrap_or(base_dir);
        rebase::rebase_settings(&mut fragment, fragment_dir, base_dir);
    }
    Ok(fragment)
}

/// The settings of the root manifest `text` with the local files it includes
/// merged in, reading each through `read`, which receives the source's path
/// joined to `base_dir`.
///
/// For a manifest as it was at another revision: ownership is not checked,
/// config dependency sources are left out, and a source `read` cannot find
/// contributes nothing.
pub fn merge_local_includes(
    text: &str,
    base_dir: &Path,
    read: impl Fn(&Path) -> io::Result<Option<String>>,
) -> Result<String, IncludeError> {
    let path = base_dir.join(crate::WORKSPACE_MANIFEST_FILENAME);
    let Some(mut root) = root_mapping_with_include(&path, text)? else {
        return Ok(text.to_string());
    };
    let index = root.remove(INCLUDE_KEY).unwrap_or(Value::Null);
    let mut fragments = Vec::new();
    for include in selector::parse_index(&index, base_dir)? {
        let IncludeSource::Local { spec, path } = &include.source else {
            continue;
        };
        let read_error = |source| IncludeError::ReadFragment { spec: spec.clone(), source };
        if let Some(text) = read(path).map_err(read_error)? {
            fragments.push(parse_fragment(&include.source, &text, base_dir)?);
        }
    }
    Ok(merge::render(merge::merge(root, fragments)))
}

#[cfg(test)]
mod tests;
