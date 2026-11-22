use pnpm_package_manifest::parse_manifest_bytes;
use pnpm_store_dir::{
    PackageFilesIndex,
    StoreDir,
    StoreIndex,
    store_index_key,
};
use std::path::{
    Path,
    PathBuf,
};

/// Read-only access to cached package files when no installation is available.
pub(super) struct StoredPackageReader {
    store_dir: StoreDir,
    index: StoreIndex,
}

impl StoredPackageReader {
    pub(super) fn open(path: &Path) -> Option<Self> {
        Some(Self {
            store_dir: StoreDir::new(path.to_path_buf()),
            index: StoreIndex::open_readonly(path).ok()?,
        })
    }

    pub(super) fn package(&self, integrity: &str, package_id: &str) -> Option<StoredPackage<'_>> {
        let files = self.index
            .get(&store_index_key(integrity, package_id))
            .ok()??;
        Some(StoredPackage { store_dir: &self.store_dir, files })
    }
}

pub(super) struct StoredPackage<'a> {
    store_dir: &'a StoreDir,
    files: PackageFilesIndex,
}

impl StoredPackage<'_> {
    pub(super) fn file_path(&self, name: &str) -> Option<PathBuf> {
        let entry = self.files.files.get(name)?;
        self.store_dir.cas_file_path_by_mode(&entry.digest, entry.mode)
    }

    pub(super) fn manifest(&self) -> Option<serde_json::Value> {
        let bytes = std::fs::read(self.file_path("package.json")?).ok()?;
        parse_manifest_bytes(&bytes).ok()
    }
}
