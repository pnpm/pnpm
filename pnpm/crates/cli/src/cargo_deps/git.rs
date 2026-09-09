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
use futures_util::{StreamExt, stream};
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
/// The transports a git dependency is fetched over. `git` runs whatever a
/// scheme outside this set names — `ext::` hands the URL to a shell — and
/// a lockfile is not a place to take a command from.
const SUPPORTED_SCHEMES: [&str; 5] = ["file", "git", "http", "https", "ssh"];

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
    pub(crate) concurrency: usize,
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
        if !SUPPORTED_SCHEMES.contains(&source.url().scheme()) {
            let repository = redact_and_sanitize(&url);
            let scheme = source.url().scheme();
            return Err(miette::miette!(
                "Cargo source {repository} asks for the {scheme} transport, which pnpm does not fetch a git dependency over",
            ));
        }
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
        concurrency,
        offline,
    } = options;
    let mut sources: BTreeMap<Arc<GitSource>, Vec<GitPackage>> = BTreeMap::new();
    for package in packages {
        sources.entry(Arc::clone(&package.source)).or_default().push(package);
    }
    let mut vendored = stream::iter(sources)
        .map(|(source, packages)| {
            let logged_methods = Arc::clone(&logged_methods);
            // Cloning a repository and copying the files out of it is
            // blocking work. Each repository is checked out once, for
            // every package it provides.
            async move {
                tokio::task::spawn_blocking(move || {
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
                .wrap_err("join git package vendoring task")?
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .concat();
    vendored.sort();
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
    let checkout_root = dunce::canonicalize(checkout.path())
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the checkout of {repository}"))?;

    for (package, slot) in missing {
        let found = checked_out.find(&package.name, &package.version)?.ok_or_else(|| {
            miette::miette!(
                "{repository} at {} holds no crate {} {}",
                source.commit,
                package.name,
                package.version,
            )
        })?;
        let cas_paths = import_package(store_dir, &checkout_root, &found, &package_dirs)?;
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
/// describes, and the directories each crate name is declared in.
struct Checkout {
    manifests: BTreeMap<PathBuf, Manifest>,
    directories_by_crate: BTreeMap<String, Vec<PathBuf>>,
}

impl Checkout {
    fn read(root: &Path) -> Result<Self> {
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
    fn find(&self, name: &str, version: &str) -> Result<Option<CheckoutPackage>> {
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
    fn package_dirs(&self) -> BTreeSet<&Path> {
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

struct ImportContext<'a> {
    store_dir: &'a StoreDir,
    /// Canonical root of the checkout. A link resolving outside it points
    /// at something the repository does not contain.
    root: &'a Path,
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
    root: &Path,
    package: &CheckoutPackage,
    package_dirs: &BTreeSet<&Path>,
) -> Result<HashMap<String, PathBuf>> {
    let context = ImportContext {
        store_dir,
        root,
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
        // A lossy name would key one file's contents under another's, and
        // the checksum manifest that keeps the vendored package honest
        // cannot spell a name that is not UTF-8 either.
        let name = entry.file_name().into_string().map_err(|name| {
            let path = dir.join(name);
            let path = path.display();
            miette::miette!("cannot vendor {path}: its name is not valid UTF-8")
        })?;
        let path = entry.path();
        match entry_kind(context.root, &entry)? {
            Some(EntryKind::Directory) => {
                if EXCLUDED_DIRECTORIES.contains(&name.as_str())
                    || context.nested.contains(path.as_path())
                {
                    continue;
                }
                import_directory(context, &path, &format!("{prefix}{name}/"), cas_paths)?;
            }
            Some(EntryKind::File { executable }) => {
                let relative = format!("{prefix}{name}");
                let cas_path = import_file(context, &path, &relative, executable)?;
                cas_paths.insert(relative, cas_path);
            }
            None => {}
        }
    }
    Ok(())
}

/// Write one file into the store, substituting the vendored manifest for
/// the committed `Cargo.toml`.
fn import_file(
    context: &ImportContext<'_>,
    path: &Path,
    relative: &str,
    executable: bool,
) -> Result<PathBuf> {
    if relative == "Cargo.toml" {
        return context
            .store_dir
            .write_cas_file(context.manifest.as_bytes(), executable)
            .into_diagnostic()
            .map(|(cas_path, _)| cas_path)
            .wrap_err_with(|| format!("store {}", path.display()));
    }
    // Streamed rather than read whole: a repository is free to carry a
    // file larger than the install's memory.
    let mut file = fs::File::open(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    context
        .store_dir
        .write_cas_file_from_reader(&mut file, executable, None)
        .into_diagnostic()
        .map(|(cas_path, _, _)| cas_path)
        .wrap_err_with(|| format!("store {}", path.display()))
}

enum EntryKind {
    Directory,
    File { executable: bool },
}

/// What a directory entry contributes to the vendored package.
///
/// A symlinked file is vendored as what it points at, which is how
/// `cargo` reads one out of a checkout. `None` for an entry the package
/// cannot carry: a link that leaves the checkout or dangles, a symlinked
/// directory, which could equally lead back into its own parent, and
/// anything that is neither a file nor a directory.
fn entry_kind(root: &Path, entry: &fs::DirEntry) -> Result<Option<EntryKind>> {
    let path = entry.path();
    if entry_file_type(entry)?.is_symlink() {
        let Ok(target) = dunce::canonicalize(&path) else {
            return Ok(None);
        };
        if !target.starts_with(root) || !target.is_file() {
            return Ok(None);
        }
    }
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("inspect {}", path.display()));
        }
    };
    Ok(if metadata.is_dir() {
        Some(EntryKind::Directory)
    } else if metadata.is_file() {
        Some(EntryKind::File { executable: is_executable(&metadata) })
    } else {
        None
    })
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;

    pnpm_fs::file_mode::is_executable(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests;
