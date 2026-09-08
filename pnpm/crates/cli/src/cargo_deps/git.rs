//! Vendor the git-sourced packages a `Cargo.lock` pins, so a workspace that
//! patches a crate with a git revision installs and builds from the store.
//!
//! The layout is the one `cargo vendor` produces for a git package: one
//! directory per package holding its source and a `.cargo-checksum.json`
//! whose `package` field is null, reached through a Cargo directory source
//! that replaces the git source. Cargo then resolves the pinned revision
//! without a fetch of its own.

use super::add_cargo_checksum;
use cargo_lock::package::GitReference;
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::PackageImportMethod;
use pnpm_deps_restorer::{ImportIndexedDirOpts, import_indexed_dir};
use pnpm_git_fetcher::{CheckoutOptions, checkout_commit};
use pnpm_network::redact_and_sanitize;
use pnpm_reporter::Reporter;
use pnpm_store_dir::StoreDir;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};

/// Directory the vendored git packages are linked into, relative to the
/// Cargo workspace root. Registry crates keep their own directory: a git
/// package and a registry crate of the same name and version are distinct
/// packages, and one directory source cannot hold both.
pub(crate) const GIT_SOURCE_DIRECTORY: [&str; 3] = [".pnpm", "crates", "git"];
/// Name of the Cargo directory source every replaced git source points at.
pub(crate) const GIT_SOURCE_NAME: &str = "pnpm-git";
/// The dependency kinds a manifest can declare, each of them inheritable.
const DEPENDENCY_KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
/// Directories `cargo` never reads a package's sources from.
const EXCLUDED_DIRECTORIES: [&str; 2] = [".git", "target"];

/// A git repository at one revision, as `Cargo.lock` spells it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GitSource {
    /// The source identifier without the commit fragment — `git+<url>`
    /// followed by the `rev`, `tag` or `branch` query the manifest asked
    /// for. Cargo reads it back as a source-replacement key.
    id: String,
    url: String,
    /// The query pair the identifier carries, absent for a dependency on
    /// the repository's default branch.
    reference: Option<(&'static str, String)>,
    /// The commit the lockfile resolved the reference to.
    commit: String,
}

/// One package `Cargo.lock` takes from a [`GitSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitPackage {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: Arc<GitSource>,
}

pub(crate) struct VendorOptions {
    pub(crate) packages: Vec<GitPackage>,
    pub(crate) store_dir: &'static StoreDir,
    pub(crate) git_shallow_hosts: &'static [String],
    pub(crate) package_import_method: PackageImportMethod,
    pub(crate) logged_methods: Arc<AtomicU8>,
    pub(crate) offline: bool,
}

impl GitSource {
    /// The source `cargo` recorded for a git-sourced package, or an error
    /// when the lockfile leaves the revision unpinned.
    pub(crate) fn from_source_id(source: &cargo_lock::SourceId) -> Result<Self> {
        let url = source.url().to_string();
        let reference = match source.git_reference() {
            Some(GitReference::Branch(branch)) => Some(("branch", branch.clone())),
            Some(GitReference::Tag(tag)) => Some(("tag", tag.clone())),
            Some(GitReference::Rev(rev)) => Some(("rev", rev.clone())),
            Some(GitReference::DefaultBranch) | None => None,
        };
        let Some(commit) = source.precise() else {
            let source = redact_and_sanitize(&source.to_string());
            return Err(miette::miette!("Cargo source {source} pins no commit"));
        };
        if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let repository = redact_and_sanitize(&url);
            return Err(miette::miette!(
                "Cargo source {repository} names {commit:?}, which is not a commit hash",
            ));
        }
        Ok(Self {
            id: source.with_precise(None).to_string(),
            url,
            reference,
            commit: commit.to_ascii_lowercase(),
        })
    }

    /// The `[source."…"]` block that sends this git source to the vendored
    /// directory, keyed the way `cargo vendor` writes it.
    pub(crate) fn config_block(&self) -> String {
        let reference = self.reference.as_ref().map_or_else(String::new, |(key, value)| {
            format!("{key} = {}\n", toml::Value::from(value.as_str()))
        });
        format!(
            "[source.{}]\ngit = {}\n{reference}replace-with = {}\n",
            toml::Value::from(self.id.as_str()),
            toml::Value::from(self.url.as_str()),
            toml::Value::from(GIT_SOURCE_NAME),
        )
    }
}

impl GitPackage {
    pub(crate) fn link_name(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }

    /// The commit identifies the source tree the package was vendored from,
    /// so it takes the place the registry checksum holds in
    /// [`super::LockedCrate::store_slot`].
    fn store_slot(&self, store_root: &Path) -> PathBuf {
        store_root
            .join("crates")
            .join(&self.name)
            .join(&self.version)
            .join(format!("git-{}", self.source.commit))
    }
}

/// Materialize every git package into the store, returning the
/// `link name → store slot` pairs the workspace directory source links.
pub(crate) async fn vendor<Reporter: self::Reporter + 'static>(
    options: VendorOptions,
) -> Result<Vec<(String, PathBuf)>> {
    let VendorOptions {
        packages,
        store_dir,
        git_shallow_hosts,
        package_import_method,
        logged_methods,
        offline,
    } = options;
    let mut sources: BTreeMap<Arc<GitSource>, Vec<GitPackage>> = BTreeMap::new();
    for package in packages {
        sources.entry(Arc::clone(&package.source)).or_default().push(package);
    }
    let mut vendored = Vec::new();
    for (source, packages) in sources {
        let logged_methods = Arc::clone(&logged_methods);
        // Cloning a repository and copying the files out of it is blocking
        // work. Each repository is checked out once, for every package it
        // provides.
        let linked = tokio::task::spawn_blocking(move || {
            vendor_source::<Reporter>(&VendorSourceOptions {
                source: &source,
                packages: &packages,
                store_dir,
                git_shallow_hosts,
                package_import_method,
                logged_methods: &logged_methods,
                offline,
            })
        })
        .await
        .into_diagnostic()
        .wrap_err("join git package vendoring task")??;
        vendored.extend(linked);
    }
    Ok(vendored)
}

struct VendorSourceOptions<'a> {
    source: &'a GitSource,
    packages: &'a [GitPackage],
    store_dir: &'a StoreDir,
    git_shallow_hosts: &'a [String],
    package_import_method: PackageImportMethod,
    logged_methods: &'a AtomicU8,
    offline: bool,
}

fn vendor_source<Reporter: self::Reporter>(
    options: &VendorSourceOptions<'_>,
) -> Result<Vec<(String, PathBuf)>> {
    let &VendorSourceOptions {
        source,
        packages,
        store_dir,
        git_shallow_hosts,
        package_import_method,
        logged_methods,
        offline,
    } = options;
    let mut linked = Vec::with_capacity(packages.len());
    let mut missing = Vec::new();
    for package in packages {
        let slot = package.store_slot(store_dir.root());
        // The checksum manifest sorts first among a crate's files, which
        // makes it the completion marker `import_indexed_dir` writes last.
        if slot.join(".cargo-checksum.json").exists() {
            linked.push((package.link_name(), slot));
        } else {
            missing.push((package, slot));
        }
    }
    if missing.is_empty() {
        return Ok(linked);
    }
    let repository = redact_and_sanitize(&source.url);
    if offline {
        return Err(miette::miette!(
            "cannot check out {repository} at {} while offline",
            source.commit,
        ));
    }

    let checkout = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err_with(|| format!("create a checkout directory for {repository}"))?;
    checkout_commit(&CheckoutOptions {
        repo: &source.url,
        commit: &source.commit,
        git_shallow_hosts,
        git_bin: None,
        dest: checkout.path(),
    })
    .map_err(|error| {
        let error = redact_and_sanitize(&error.to_string());
        miette::miette!("{error}")
    })
    .wrap_err_with(|| format!("check out {repository} at {}", source.commit))?;
    let checked_out = Checkout::read(checkout.path())?;
    let package_dirs = checked_out.package_dirs();

    for (package, slot) in missing {
        let found = checked_out.find(&package.name, &package.version)?.ok_or_else(|| {
            miette::miette!(
                "{repository} at {} holds no crate {} {}",
                source.commit,
                package.name,
                package.version,
            )
        })?;
        let cas_paths = import_package(store_dir, &found, &package_dirs)?;
        import_indexed_dir::<Reporter>(
            logged_methods,
            package_import_method,
            &slot,
            &cas_paths,
            ImportIndexedDirOpts {
                force: true,
                safe_to_skip: true,
                ..ImportIndexedDirOpts::default()
            },
        )
        .into_diagnostic()
        .wrap_err_with(|| format!("materialize cargo package at {}", slot.display()))?;
        linked.push((package.link_name(), slot));
    }
    Ok(linked)
}

/// A package found in a git checkout, with the manifest the vendored
/// directory holds in place of the one the repository committed.
struct CheckoutPackage {
    dir: PathBuf,
    manifest: String,
}

/// A `Cargo.toml` in a git checkout, as text and as a document.
struct Manifest {
    text: String,
    document: toml::Table,
}

/// The manifests a git checkout holds, keyed by the directory each one
/// describes.
struct Checkout {
    manifests: BTreeMap<PathBuf, Manifest>,
}

impl Checkout {
    fn read(root: &Path) -> Result<Self> {
        let mut manifests = BTreeMap::new();
        collect_manifests(root, &mut manifests)?;
        Ok(Self { manifests })
    }

    /// The directory holding `name` at `version`, if the checkout has one.
    /// A package's name is never inherited, so only the manifests already
    /// naming this crate are resolved against their workspace.
    fn find(&self, name: &str, version: &str) -> Result<Option<CheckoutPackage>> {
        for (dir, manifest) in &self.manifests {
            if package_name(&manifest.document) != Some(name) {
                continue;
            }
            let workspace = workspace_manifest(dir, &manifest.document, &self.manifests);
            let package = vendored_package(manifest, workspace)
                .wrap_err_with(|| format!("read {}", dir.join("Cargo.toml").display()))?;
            if package.version != version {
                continue;
            }
            return Ok(Some(CheckoutPackage { dir: dir.clone(), manifest: package.manifest }));
        }
        Ok(None)
    }

    /// Every directory in the checkout that `cargo` reads a package from.
    fn package_dirs(&self) -> BTreeSet<&Path> {
        self.manifests
            .iter()
            .filter(|(_, manifest)| package_name(&manifest.document).is_some())
            .map(|(dir, _)| dir.as_path())
            .collect()
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

fn read_directory(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    fs::read_dir(dir)
        .and_then(Iterator::collect::<io::Result<Vec<_>>>)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", dir.display()))
}

fn entry_file_type(entry: &fs::DirEntry) -> Result<fs::FileType> {
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
        return Some(&manifests.get(&dir.join(path.as_str()?))?.document);
    }
    dir.ancestors().skip(1).find_map(|ancestor| {
        let document = &manifests.get(ancestor)?.document;
        document.contains_key("workspace").then_some(document)
    })
}

/// A manifest resolved against its workspace, and the version `cargo`
/// reads from it.
#[derive(Debug)]
struct VendoredPackage {
    version: String,
    manifest: String,
}

/// Resolve every `workspace = true` inheritance marker against `workspace`
/// and drop the `[workspace]` table, so the package's directory carries
/// everything its manifest refers to.
///
/// The manifest text is kept verbatim when it inherits nothing, which
/// preserves a single-crate repository's own formatting.
fn vendored_package(
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
    let mut inherited = false;
    for kind in DEPENDENCY_KINDS {
        if let Some(table) = document.get_mut(kind).and_then(toml::Value::as_table_mut) {
            inherited |= inherit_dependency_table(table, workspace)?;
        }
    }
    if let Some(targets) = document.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (_, target) in targets.iter_mut() {
            for kind in DEPENDENCY_KINDS {
                if let Some(table) = target.get_mut(kind).and_then(toml::Value::as_table_mut) {
                    inherited |= inherit_dependency_table(table, workspace)?;
                }
            }
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
        let local = declaration.as_table().expect("an inheriting entry is a table");
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
        *declaration = merged.into();
        inherited = true;
    }
    Ok(inherited)
}

/// Whether a manifest entry defers to the workspace (`x.workspace = true`).
fn inherits_from_workspace(value: &toml::Value) -> bool {
    value.get("workspace").and_then(toml::Value::as_bool).unwrap_or(false)
}

struct ImportContext<'a> {
    store_dir: &'a StoreDir,
    /// Packages nested inside the one being imported. They are vendored
    /// into directories of their own, the way `cargo` keeps a workspace
    /// member out of its root package's file list.
    nested: BTreeSet<&'a Path>,
    manifest: &'a str,
}

/// Write a package's files into the content-addressed store, with the
/// vendored manifest in place of the committed one.
fn import_package(
    store_dir: &StoreDir,
    package: &CheckoutPackage,
    package_dirs: &BTreeSet<&Path>,
) -> Result<HashMap<String, PathBuf>> {
    let context = ImportContext {
        store_dir,
        nested: package_dirs
            .iter()
            .copied()
            .filter(|dir| *dir != package.dir && dir.starts_with(&package.dir))
            .collect(),
        manifest: &package.manifest,
    };
    let mut cas_paths = HashMap::new();
    import_directory(&context, &package.dir, "", &mut cas_paths)?;
    add_cargo_checksum(store_dir, &mut cas_paths, None)?;
    Ok(cas_paths)
}

fn import_directory(
    context: &ImportContext<'_>,
    dir: &Path,
    prefix: &str,
    cas_paths: &mut HashMap<String, PathBuf>,
) -> Result<()> {
    for entry in read_directory(dir)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Symlinks are not followed: a target outside the checkout would
        // put a file the repository does not contain into the store.
        let file_type = entry_file_type(&entry)?;
        let path = entry.path();
        if file_type.is_dir() {
            if EXCLUDED_DIRECTORIES.contains(&name.as_ref())
                || context.nested.contains(path.as_path())
            {
                continue;
            }
            import_directory(context, &path, &format!("{prefix}{name}/"), cas_paths)?;
        } else if file_type.is_file() {
            let relative = format!("{prefix}{name}");
            let contents = if relative == "Cargo.toml" {
                context.manifest.as_bytes().to_vec()
            } else {
                fs::read(&path)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("read {}", path.display()))?
            };
            let (cas_path, _) = context
                .store_dir
                .write_cas_file(&contents, is_executable(&entry)?)
                .into_diagnostic()
                .wrap_err_with(|| format!("store {}", path.display()))?;
            cas_paths.insert(relative, cas_path);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(entry: &fs::DirEntry) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = entry
        .metadata()
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect {}", entry.path().display()))?;
    Ok(pnpm_fs::file_mode::is_executable(metadata.permissions().mode()))
}

#[cfg(not(unix))]
fn is_executable(_entry: &fs::DirEntry) -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests;
