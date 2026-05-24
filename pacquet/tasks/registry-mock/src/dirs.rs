use pipe_trait::Pipe;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

pub fn workspace_root() -> &'static Path {
    static WORKSPACE_ROOT: OnceLock<PathBuf> = OnceLock::new();
    WORKSPACE_ROOT.get_or_init(|| {
        let output = env!("CARGO")
            .pipe(Command::new)
            .arg("locate-project")
            .arg("--workspace")
            .arg("--message-format=plain")
            .output()
            .expect("cargo locate-project");
        assert!(
            output.status.success(),
            "Command `cargo locate-project` exits with non-zero status code",
        );
        output
            .stdout
            .pipe(String::from_utf8)
            .expect("convert stdout to UTF-8")
            .trim_end()
            .pipe(Path::new)
            .parent()
            .expect("parent of root manifest")
            .to_path_buf()
    })
}

pub fn registry_mock() -> &'static Path {
    static REGISTRY_MOCK: OnceLock<PathBuf> = OnceLock::new();
    REGISTRY_MOCK
        .get_or_init(|| workspace_root().join("pacquet").join("tasks").join("registry-mock"))
}

/// The verdaccio-shaped storage that `@pnpm/registry-mock`'s published
/// npm tarball ships under `registry/storage-cache/`. We hand this to
/// `pnpm-registry --static --storage <path>` instead of running
/// verdaccio ourselves; the storage already contains the fixture
/// packages pacquet's tests rely on, so no `prepare` step is needed.
pub fn registry_mock_storage() -> &'static Path {
    static STORAGE: OnceLock<PathBuf> = OnceLock::new();
    STORAGE.get_or_init(|| {
        registry_mock()
            .join("node_modules")
            .join("@pnpm")
            .join("registry-mock")
            .join("registry")
            .join("storage-cache")
    })
}
