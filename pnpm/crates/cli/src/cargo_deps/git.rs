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
use manifest::{Checkout, CheckoutPackage, entry_file_type, read_directory};
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
    let sources = group_git_packages(packages);
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
    let mut linked = Vec::with_capacity(options.packages.len());
    let mut missing = Vec::new();
    for package in options.packages {
        let slot = package.store_slot(options.store_dir.root());
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
    let repository = redact_and_sanitize(&options.source.url);
    let checkout = checkout_source(options)?;
    let checked_out = Checkout::read(checkout.path())?;
    let package_dirs = checked_out.package_dirs();
    let checkout_root = dunce::canonicalize(checkout.path())
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the checkout of {repository}"))?;

    for (package, slot) in missing {
        let found = require_git_package(&checked_out, package, options.source, &repository)?;
        let cas_paths = import_package(options.store_dir, &checkout_root, &found, &package_dirs)?;
        import_indexed_dir::<Reporter>(
            options.logged_methods,
            options.package_import_method,
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

fn group_git_packages(packages: Vec<GitPackage>) -> BTreeMap<Arc<GitSource>, Vec<GitPackage>> {
    let mut sources: BTreeMap<Arc<GitSource>, Vec<GitPackage>> = BTreeMap::new();
    for package in packages {
        sources.entry(Arc::clone(&package.source)).or_default().push(package);
    }
    sources
}

fn checkout_source(options: &VendorSourceOptions<'_>) -> Result<tempfile::TempDir> {
    let repository = redact_and_sanitize(&options.source.url);
    if options.offline {
        return Err(miette::miette!(
            "cannot check out {repository} at {} while offline",
            options.source.commit,
        ));
    }

    let checkout = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err_with(|| format!("create a checkout directory for {repository}"))?;
    checkout_commit(&CheckoutOptions {
        repo: &options.source.url,
        commit: &options.source.commit,
        git_shallow_hosts: options.git_shallow_hosts,
        git_bin: None,
        dest: checkout.path(),
    })
    .map_err(|error| {
        let error = redact_and_sanitize(&error.to_string());
        miette::miette!("{error}")
    })
    .wrap_err_with(|| format!("check out {repository} at {}", options.source.commit))?;
    Ok(checkout)
}

fn require_git_package(
    checked_out: &Checkout,
    package: &GitPackage,
    source: &GitSource,
    repository: &str,
) -> Result<CheckoutPackage> {
    checked_out.find(&package.name, &package.version)?.ok_or_else(|| {
        miette::miette!(
            "{repository} at {} holds no crate {} {}",
            source.commit,
            package.name,
            package.version,
        )
    })
}

mod manifest;
