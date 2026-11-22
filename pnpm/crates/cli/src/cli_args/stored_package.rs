use pnpm_package_manifest::parse_manifest_bytes;
use pnpm_store_dir::{
    PackageFilesIndex,
    SharedReadonlyStoreIndex,
    StoreDir,
    StoreIndex,
};
use std::{
    path::PathBuf,
    sync::OnceLock,
};

/// Read-only access to cached package files when no installation is available.
pub(super) struct StoredPackageReader {
    store_dir: StoreDir,
    index: SharedReadonlyStoreIndex,
}

impl StoredPackageReader {
    pub(super) fn open(path: PathBuf, frozen_store: bool) -> Option<Self> {
        let store_dir = StoreDir::new(path);
        let index = StoreIndex::shared_for(&store_dir, frozen_store)?;
        Some(Self { store_dir, index })
    }

    pub(super) fn package(&self, key: String) -> StoredPackage<'_> {
        StoredPackage { reader: self, key, files: OnceLock::new() }
    }
}

pub(super) struct StoredPackage<'a> {
    reader: &'a StoredPackageReader,
    key: String,
    files: OnceLock<Option<PackageFilesIndex>>,
}

impl StoredPackage<'_> {
    pub(super) fn file_path(&self, name: &str) -> Option<PathBuf> {
        let files = self.files.get_or_init(|| {
            self.reader.index
                .lock()
                .expect("store index lock poisoned")
                .get(&self.key)
                .ok()
                .flatten()
        });
        let entry = files.as_ref()?.files.get(name)?;
        self.reader.store_dir.cas_file_path_by_mode(&entry.digest, entry.mode)
    }

    pub(super) fn manifest(&self) -> Option<serde_json::Value> {
        let bytes = std::fs::read(self.file_path("package.json")?).ok()?;
        parse_manifest_bytes(&bytes).ok()
    }
}
