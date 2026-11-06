use crate::{
    GENERATED_DIR, PACKAGES_DIR, ensure_storage_for_fingerprint, fixture_fingerprint, target_dir,
    workspace_root,
};
use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Debug, Clone)]
pub struct FixtureGeneration {
    fingerprint: String,
    storage: PathBuf,
}

impl FixtureGeneration {
    #[must_use]
    pub fn of(packages: &Path, generated: &Path) -> Self {
        let fingerprint = fixture_fingerprint(packages);
        let storage = generated.join("storage").join(&fingerprint);
        ensure_storage_for_fingerprint(packages, generated, &storage);
        Self { fingerprint, storage }
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[must_use]
    pub fn storage(&self) -> &Path {
        &self.storage
    }
}

#[must_use]
pub fn current() -> &'static FixtureGeneration {
    static CURRENT: LazyLock<FixtureGeneration> = LazyLock::new(|| {
        let workspace = workspace_root();
        FixtureGeneration::of(
            &workspace.join(PACKAGES_DIR),
            &target_dir(&workspace).join(GENERATED_DIR),
        )
    });
    &CURRENT
}
