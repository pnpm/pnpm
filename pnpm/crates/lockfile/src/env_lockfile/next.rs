//! The env document in the next format: the shape the main document has,
//! with `configDependencies` and `packageManagerDependencies` as the root
//! importer's dependency groups, and one `packages` entry per package.
//!
//! The conversion is the main document's. The env document becomes a
//! lockfile with one importer whose `dependencies` are both groups, goes
//! through the same writer and reader, and is split back into its groups
//! by the names each group had.

use super::{EnvImporterSnapshot, EnvLockfile, SpecifierAndResolution};
use crate::{
    ImporterDepVersion, LoadLockfileError, Lockfile, LockfileVersion, NextFormatError, PkgName,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SaveLockfileError,
    implied_version::injected_paths,
    json_document::{from_json_str, is_json_document},
    next_format::{
        Implied, LinkedImporters, NextImporter, NextImporterDeps, NextPackage, NextPackageOut,
        NextSections, RestoreContext, merge_packages, restore_sections, split_packages,
    },
    serialize_yaml,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

/// The env document as written and read. `Package` is the entry as
/// written, borrowing, or as read.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "Package: Deserialize<'de>"))]
struct NextEnvDocument<Package> {
    lockfile_version: LockfileVersion<9>,
    #[serde(rename = "projects")]
    importers: BTreeMap<String, NextEnvImporter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    packages: Option<HashMap<crate::PackageKey, Package>>,
}

/// The root importer: the two groups, each written as an importer's
/// dependencies are.
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NextEnvImporter {
    #[serde(default, serialize_with = "serialize_yaml::sorted_map")]
    config_dependencies: NextImporterDeps,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_yaml::sorted_map_opt"
    )]
    package_manager_dependencies: Option<NextImporterDeps>,
}

/// The document text, under `version`.
pub(super) fn to_text(
    env: &EnvLockfile,
    version: LockfileVersion<9>,
) -> Result<String, SaveLockfileError> {
    let root = env.importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .cloned()
        .unwrap_or_default();
    let lockfile = as_lockfile(env, &root, version).map_err(SaveLockfileError::NextFormat)?;
    let NextSections { importers, snapshots } =
        lockfile.to_next_sections().map_err(SaveLockfileError::NextFormat)?;
    let packages = merge_packages(lockfile.packages.as_ref(), snapshots);
    let injected = injected_paths(packages.keys());
    let linked = LinkedImporters::new();
    let implied = Implied { catalogs: None, injected: &injected, linked: &linked };
    let dependencies = importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .map(|importer| importer.collapsed(EnvLockfile::ROOT_IMPORTER_KEY, implied))
        .and_then(|importer| importer.dependencies)
        .unwrap_or_default();
    let importer = split_groups(dependencies, &package_manager_names(&root));
    let document = NextEnvDocument::<NextPackageOut<'_>> {
        lockfile_version: version,
        importers: BTreeMap::from([(EnvLockfile::ROOT_IMPORTER_KEY.to_string(), importer)]),
        packages: (!packages.is_empty()).then_some(packages),
    };
    serialize_yaml::to_string(&document).map_err(SaveLockfileError::SerializeYaml)
}

/// The env document `text` as the env lockfile it describes.
pub(super) fn from_text(text: &str, path: &Path) -> Result<EnvLockfile, LoadLockfileError> {
    let document: NextEnvDocument<NextPackage> = if is_json_document(text) {
        from_json_str(text)
            .map_err(|source| LoadLockfileError::ParseYaml {
                path: path.to_path_buf(),
                reason: source.to_string(),
            })?
    } else {
        serde_saphyr::from_str(text).map_err(|source| LoadLockfileError::parse_yaml(path, &source))?
    };
    let next_format = |source| LoadLockfileError::next_format(path, &source);
    let (packages, snapshots) = match document.packages {
        Some(merged) => split_packages(merged).map_err(next_format)?,
        None => (None, None),
    };
    let root = document.importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .cloned()
        .unwrap_or_default();
    let package_manager_names: HashSet<String> = root
        .package_manager_dependencies
        .iter()
        .flatten()
        .map(|(name, _)| name.to_string())
        .collect();
    let importer = resolve_root(root, packages.as_ref()).map_err(next_format)?;
    let (resolved, restored_snapshots) = restore(importer, snapshots, packages.as_ref());
    Ok(EnvLockfile {
        lockfile_version: document.lockfile_version.to_string(),
        importers: HashMap::from([(
            EnvLockfile::ROOT_IMPORTER_KEY.to_string(),
            env_importer(resolved, &package_manager_names),
        )]),
        packages: packages.unwrap_or_default(),
        snapshots: restored_snapshots.unwrap_or_default(),
    })
}

/// The root importer's two groups as one importer with every version
/// written out.
fn resolve_root(
    root: NextEnvImporter,
    packages: Option<&HashMap<crate::PackageKey, crate::PackageMetadata>>,
) -> Result<NextImporter, NextFormatError> {
    let injected = injected_paths(packages.into_iter().flat_map(HashMap::keys));
    let linked = LinkedImporters::new();
    let implied = Implied { catalogs: None, injected: &injected, linked: &linked };
    let mut dependencies = root.config_dependencies;
    dependencies.extend(root.package_manager_dependencies.unwrap_or_default());
    NextImporter { dependencies: Some(dependencies), ..NextImporter::default() }.resolve_specifiers(
        EnvLockfile::ROOT_IMPORTER_KEY,
        implied,
    )
}

/// Today's root dependencies and peer-suffixed snapshots, rebuilt from the
/// next format's.
fn restore(
    importer: NextImporter,
    snapshots: Option<crate::NextSnapshots>,
    packages: Option<&HashMap<crate::PackageKey, crate::PackageMetadata>>,
) -> (ResolvedDependencyMap, Option<HashMap<crate::PackageKey, crate::SnapshotEntry>>) {
    let sections = NextSections {
        importers: HashMap::from([(EnvLockfile::ROOT_IMPORTER_KEY.to_string(), importer)]),
        snapshots,
    };
    let context = RestoreContext { packages, settings: None, patched_dependencies: None };
    let mut restored = restore_sections(&sections, context);
    let resolved = restored.importers
        .remove(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|project| project.dependencies)
        .unwrap_or_default();
    (resolved, restored.snapshots)
}

/// The env lockfile as a main-shaped lockfile, whose one importer's
/// dependencies are both groups of `root`.
fn as_lockfile(
    env: &EnvLockfile,
    root: &EnvImporterSnapshot,
    version: LockfileVersion<9>,
) -> Result<Lockfile, NextFormatError> {
    let mut dependencies = ResolvedDependencyMap::new();
    let groups = root.config_dependencies
        .iter()
        .chain(root.package_manager_dependencies.iter().flatten());
    for (name, resolution) in groups {
        let parsed = PkgName::parse(name).map_err(|_| not_representable(name))?;
        let version = resolution.version
            .parse::<ImporterDepVersion>()
            .map_err(|_| not_representable(&resolution.version))?;
        let spec = ResolvedDependencySpec { specifier: resolution.specifier.clone(), version };
        if dependencies.insert(parsed, spec).is_some() {
            return Err(not_representable(&format!("{name} is in both groups")));
        }
    }
    let project =
        ProjectSnapshot { dependencies: Some(dependencies), ..ProjectSnapshot::default() };
    Ok(Lockfile {
        lockfile_version: version,
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::from([(EnvLockfile::ROOT_IMPORTER_KEY.to_string(), project)]),
        packages: Some(env.packages.clone()),
        snapshots: Some(env.snapshots.clone()),
        time: None,
        extra: crate::LockfileExtra::default(),
    })
}

fn not_representable(detail: &str) -> NextFormatError {
    NextFormatError::NotRepresentable { detail: format!("env document, {detail}") }
}

fn package_manager_names(root: &EnvImporterSnapshot) -> HashSet<String> {
    root.package_manager_dependencies
        .iter()
        .flatten()
        .map(|(name, _)| name.clone())
        .collect()
}

/// `dependencies` split back into the two groups by the package-manager
/// names.
fn split_groups(
    dependencies: NextImporterDeps,
    package_manager_names: &HashSet<String>,
) -> NextEnvImporter {
    let mut importer = NextEnvImporter::default();
    for (name, dependency) in dependencies {
        if package_manager_names.contains(&name.to_string()) {
            importer.package_manager_dependencies.get_or_insert_default().insert(name, dependency);
        } else {
            importer.config_dependencies.insert(name, dependency);
        }
    }
    importer
}

/// The restored dependencies as the env lockfile's groups.
fn env_importer(
    resolved: ResolvedDependencyMap,
    package_manager_names: &HashSet<String>,
) -> EnvImporterSnapshot {
    let mut importer = EnvImporterSnapshot::default();
    for (name, spec) in resolved {
        let name = name.to_string();
        let resolution =
            SpecifierAndResolution { specifier: spec.specifier, version: spec.version.to_string() };
        if package_manager_names.contains(&name) {
            importer.package_manager_dependencies.get_or_insert_default().insert(name, resolution);
        } else {
            importer.config_dependencies.insert(name, resolution);
        }
    }
    importer
}
