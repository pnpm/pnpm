use crate::{CAS_MANIFEST_FILENAME, StoreDir, StoreLoaderManifest, get_registered_projects};
use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct LoaderReferences {
    pub files: HashSet<PathBuf>,
    pub package_roots: Vec<PathBuf>,
}

pub(crate) fn loader_references(store: &StoreDir) -> io::Result<LoaderReferences> {
    let projects = get_registered_projects(store).map_err(io::Error::other)?;
    if projects.is_empty() {
        return Ok(LoaderReferences::default());
    }
    let canonical_store = dunce::canonicalize(store.root())?;
    let mut references = LoaderReferences::default();
    for project in projects {
        let Some(mut manifest) = read_manifest(&project.join(CAS_MANIFEST_FILENAME))? else {
            continue;
        };
        if !same_store(&project.join(&manifest.store_dir), &canonical_store)? {
            continue;
        }
        manifest.store_dir = store.root().to_path_buf();
        add_references(&mut references, manifest, &project)?;
    }
    Ok(references)
}

fn read_manifest(path: &Path) -> io::Result<Option<StoreLoaderManifest>> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(io::Error::other(format!("Cannot read {}: {error}", path.display())));
        }
    };
    let manifest: StoreLoaderManifest = serde_json::from_slice(&contents)
        .map_err(|error| io::Error::other(format!("Cannot read {}: {error}", path.display())))?;
    if manifest.version != 1 {
        return Err(io::Error::other(format!(
            "Unsupported store manifest version at {}",
            path.display(),
        )));
    }
    Ok(Some(manifest))
}

fn same_store(path: &Path, canonical_store: &Path) -> io::Result<bool> {
    match dunce::canonicalize(path) {
        Ok(path) => Ok(path == canonical_store),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn add_references(
    references: &mut LoaderReferences,
    manifest: StoreLoaderManifest,
    project: &Path,
) -> io::Result<()> {
    let files_dir = manifest.store_dir.join("files");
    for package in manifest.packages.into_values() {
        if package.resolution.as_deref() == Some("node")
            && let Some(root) = package.root
        {
            references.package_roots.push(project.join(root));
        }
        for hash in package.files.into_iter().flat_map(BTreeMap::into_values) {
            references.files.insert(blob_path(&files_dir, &hash)?);
        }
    }
    Ok(())
}

fn blob_path(files_dir: &Path, hash: &str) -> io::Result<PathBuf> {
    let digest = hash.strip_suffix("-exec").unwrap_or(hash);
    if digest.len() != 128
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(io::Error::other(format!(
            "Invalid CAS file hash in store manifest for {}",
            files_dir.display(),
        )));
    }
    Ok(files_dir
        .join(&hash[..2])
        .join(&hash[2..]))
}

#[cfg(test)]
mod tests;
