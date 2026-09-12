use super::{
    BTreeMap, BTreeSet, DEPENDENCY_KINDS, EXCLUDED_DIRECTORIES, IntoDiagnostic, Path, PathBuf,
    Result, fs, io,
};
use miette::WrapErr;

/// A package found in a git checkout, with the manifest the vendored
/// directory holds in place of the one the repository committed.
pub(super) struct CheckoutPackage {
    pub(super) dir: PathBuf,
    pub(super) manifest: String,
}

/// A `Cargo.toml` in a git checkout, as text and as a document.
pub(super) struct Manifest {
    pub(super) text: String,
    pub(super) document: toml::Table,
}

/// The manifests a git checkout holds, keyed by the directory each one
/// describes, and the directories each crate name is declared in.
pub(super) struct Checkout {
    pub(super) manifests: BTreeMap<PathBuf, Manifest>,
    pub(super) directories_by_crate: BTreeMap<String, Vec<PathBuf>>,
}

impl Checkout {
    pub(super) fn read(root: &Path) -> Result<Self> {
        let mut manifests = BTreeMap::new();
        collect_manifests(root, &mut manifests)?;
        let mut directories_by_crate: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for (dir, manifest) in &manifests {
            if let Some(name) = package_name(&manifest.document) {
                directories_by_crate.entry(name.to_string()).or_default().push(dir.clone());
            }
        }
        Ok(Self { manifests, directories_by_crate })
    }

    /// The directory holding `name` at `version`, if the checkout has one.
    /// A package's name is never inherited, so only the manifests already
    /// naming this crate are resolved against their workspace.
    pub(super) fn find(&self, name: &str, version: &str) -> Result<Option<CheckoutPackage>> {
        // A repository may hold another crate of the same name that it
        // cannot describe on its own — a fixture, or a member of a
        // workspace the checkout does not reach. Only the one the
        // lockfile asks for has to be readable, so a candidate that is
        // not it takes its error out of the way.
        let mut unreadable = None;
        for dir in self.directories_by_crate.get(name).map(Vec::as_slice).unwrap_or_default() {
            let manifest = &self.manifests[dir];
            let workspace = workspace_manifest(dir, &manifest.document, &self.manifests);
            let package = match vendored_package(manifest, workspace)
                .wrap_err_with(|| format!("read {}", dir.join("Cargo.toml").display()))
            {
                Ok(package) => package,
                Err(error) => {
                    unreadable.get_or_insert(error);
                    continue;
                }
            };
            if package.version != version {
                continue;
            }
            return Ok(Some(CheckoutPackage { dir: dir.clone(), manifest: package.manifest }));
        }
        unreadable.map_or(Ok(None), Err)
    }

    /// Every directory in the checkout that `cargo` reads a package from.
    pub(super) fn package_dirs(&self) -> BTreeSet<&Path> {
        self.directories_by_crate.values().flatten().map(PathBuf::as_path).collect()
    }
}

fn package_name(document: &toml::Table) -> Option<&str> {
    document.get("package")?.get("name")?.as_str()
}

/// Read the `Cargo.toml` of every directory under `dir`.
///
/// A manifest that does not parse is passed over rather than failing the
/// checkout, the way `cargo` reads the packages of a git repository: a
/// repository is free to carry a fixture manifest no package is built
/// from.
fn collect_manifests(dir: &Path, manifests: &mut BTreeMap<PathBuf, Manifest>) -> Result<()> {
    let manifest_path = dir.join("Cargo.toml");
    match fs::read_to_string(&manifest_path) {
        Ok(text) => {
            if let Ok(document) = toml::from_str(&text) {
                manifests.insert(dir.to_path_buf(), Manifest { text, document });
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", manifest_path.display()));
        }
    }
    for entry in read_directory(dir)? {
        // Symlinked directories are left alone: one can leave the checkout,
        // and a loop through one would not terminate.
        if entry_file_type(&entry)?.is_dir()
            && !EXCLUDED_DIRECTORIES.contains(&entry.file_name().to_string_lossy().as_ref())
        {
            collect_manifests(&entry.path(), manifests)?;
        }
    }
    Ok(())
}

pub(super) fn read_directory(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    fs::read_dir(dir)
        .and_then(Iterator::collect::<io::Result<Vec<_>>>)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", dir.display()))
}

pub(super) fn entry_file_type(entry: &fs::DirEntry) -> Result<fs::FileType> {
    entry
        .file_type()
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect {}", entry.path().display()))
}

/// The manifest whose `[workspace]` table `dir`'s package inherits from,
/// following `cargo`'s rule: the `package.workspace` path when set, and
/// otherwise the closest ancestor within the checkout that declares one.
fn workspace_manifest<'a>(
    dir: &Path,
    document: &'a toml::Table,
    manifests: &'a BTreeMap<PathBuf, Manifest>,
) -> Option<&'a toml::Table> {
    if document.contains_key("workspace") {
        return Some(document);
    }
    if let Some(path) = document.get("package").and_then(|package| package.get("workspace")) {
        // `Path::join` keeps `..` verbatim, and the checkout was walked
        // into paths that carry none.
        let root = pnpm_fs::lexical_normalize(&dir.join(path.as_str()?));
        return Some(&manifests.get(&root)?.document);
    }
    dir.ancestors().skip(1).find_map(|ancestor| {
        let document = &manifests.get(ancestor)?.document;
        document.contains_key("workspace").then_some(document)
    })
}

/// A manifest resolved against its workspace, and the version `cargo`
/// reads from it.
#[derive(Debug)]
pub(super) struct VendoredPackage {
    pub(super) version: String,
    pub(super) manifest: String,
}

/// Resolve every `workspace = true` inheritance marker against `workspace`
/// and drop the `[workspace]` table, so the package's directory carries
/// everything its manifest refers to.
///
/// The manifest text is kept verbatim when it inherits nothing, which
/// preserves a single-crate repository's own formatting.
pub(super) fn vendored_package(
    manifest: &Manifest,
    workspace: Option<&toml::Table>,
) -> Result<VendoredPackage> {
    let workspace = workspace.and_then(|manifest| manifest.get("workspace")?.as_table());
    let mut vendored = manifest.document.clone();
    let mut inherited = false;
    if let Some(package) = vendored.get_mut("package").and_then(toml::Value::as_table_mut) {
        let workspace_package =
            workspace.and_then(|workspace| workspace.get("package")?.as_table());
        for (field, value) in package.iter_mut() {
            if !inherits_from_workspace(value) {
                continue;
            }
            *value = workspace_package
                .and_then(|package| package.get(field))
                .ok_or_else(|| {
                    miette::miette!("the workspace declares no `package.{field}` to inherit")
                })?
                .clone();
            inherited = true;
        }
    }
    inherited |= inherit_dependencies(&mut vendored, workspace)?;
    if let Some(lints) = vendored.get_mut("lints").filter(|lints| inherits_from_workspace(lints)) {
        *lints = workspace
            .and_then(|workspace| workspace.get("lints"))
            .ok_or_else(|| miette::miette!("the workspace declares no `lints` to inherit"))?
            .clone();
        inherited = true;
    }
    // A `[workspace]` table left in place names members the vendored
    // directory does not hold, and `cargo` reads it as a workspace root.
    let had_workspace = vendored.remove("workspace").is_some();

    let version = vendored
        .get("package")
        .and_then(|package| package.get("version")?.as_str())
        .ok_or_else(|| miette::miette!("the package declares no version"))?
        .to_string();
    let text = if inherited || had_workspace {
        toml::to_string(&vendored).into_diagnostic().wrap_err("serialize Cargo.toml")?
    } else {
        manifest.text.clone()
    };
    Ok(VendoredPackage { version, manifest: text })
}

/// Resolve the inheritance markers in every dependency table the manifest
/// declares, including the per-target ones. Reports whether anything was
/// inherited.
fn inherit_dependencies(
    document: &mut toml::Table,
    workspace: Option<&toml::Table>,
) -> Result<bool> {
    let mut inherited = inherit_dependency_kinds(document, workspace)?;
    if let Some(targets) = document.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (_, target) in targets.iter_mut() {
            let Some(target) = target.as_table_mut() else { continue };
            inherited |= inherit_dependency_kinds(target, workspace)?;
        }
    }
    Ok(inherited)
}

/// Resolve the inheritance markers in each of one table's dependency
/// kinds — the manifest root, or one `[target.<cfg>]` section.
fn inherit_dependency_kinds(
    table: &mut toml::Table,
    workspace: Option<&toml::Table>,
) -> Result<bool> {
    let mut inherited = false;
    for kind in DEPENDENCY_KINDS {
        if let Some(dependencies) = table.get_mut(kind).and_then(toml::Value::as_table_mut) {
            inherited |= inherit_dependency_table(dependencies, workspace)?;
        }
    }
    Ok(inherited)
}

/// Replace each `dep.workspace = true` entry with the workspace's
/// declaration of that dependency, keeping the `features`, `optional`,
/// `public` and `default-features` the member added.
fn inherit_dependency_table(
    dependencies: &mut toml::Table,
    workspace: Option<&toml::Table>,
) -> Result<bool> {
    let mut inherited = false;
    for (name, declaration) in dependencies.iter_mut() {
        if !inherits_from_workspace(declaration) {
            continue;
        }
        let declared =
            workspace.and_then(|workspace| workspace.get("dependencies")?.get(name)).ok_or_else(
                || miette::miette!("the workspace declares no dependency {name} to inherit"),
            )?;
        *declaration = merge_workspace_declaration(name, declared, declaration)?.into();
        inherited = true;
    }
    Ok(inherited)
}

/// The workspace's declaration of `name` with the member's own
/// `features`, `optional`, `public` and `default-features` folded in.
fn merge_workspace_declaration(
    name: &str,
    declared: &toml::Value,
    local: &toml::Value,
) -> Result<toml::Table> {
    let mut merged = match declared {
        toml::Value::String(version) => {
            toml::Table::from_iter([("version".to_string(), version.as_str().into())])
        }
        toml::Value::Table(table) => table.clone(),
        _ => {
            return Err(miette::miette!(
                "the workspace declares dependency {name} as neither a version nor a table",
            ));
        }
    };
    let local = local.as_table().expect("an inheriting entry is a table");
    for key in ["optional", "public", "default-features"] {
        if let Some(value) = local.get(key) {
            merged.insert(key.to_string(), value.clone());
        }
    }
    // A member's features add to the workspace's rather than replace them.
    let features = merged
        .get("features")
        .into_iter()
        .chain(local.get("features"))
        .filter_map(toml::Value::as_array)
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    if !features.is_empty() {
        merged.insert("features".to_string(), features.into());
    }
    Ok(merged)
}

/// Whether a manifest entry defers to the workspace (`x.workspace = true`).
fn inherits_from_workspace(value: &toml::Value) -> bool {
    value.get("workspace").and_then(toml::Value::as_bool).unwrap_or(false)
}
