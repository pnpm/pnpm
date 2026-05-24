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
/// npm tarball ships under `registry/storage-cache/`. We don't serve
/// directly from here — see [`runtime_storage`] for why — but we
/// seed [`runtime_storage`] from it on every launch.
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

/// Stable cache path we hand to `pnpm-registry --storage` (instead
/// of [`registry_mock_storage`]). Two reasons it has to be separate
/// and stable:
///
/// 1. `pnpm-registry` writes proxy-mode cache entries (the ~2.3k
///    unscoped npm packages the benchmark lockfile pulls) into
///    `--storage`. If we pointed at the registry-mock package's
///    install dir we'd pollute `node_modules` and lose every cached
///    entry when `pnpm install` recreates it.
/// 2. CI caches this path across runs
///    (`.github/workflows/pacquet-integrated-benchmark.yml`). Without
///    that, cold-cache scenarios pay a full 2.3k-packument fetch
///    from npmjs on every run.
///
/// The path can be overridden via the `PNPM_REGISTRY_STORAGE` env
/// var. Defaults to `$HOME/.cache/pnpm-registry/storage`.
pub fn runtime_storage() -> &'static Path {
    static STORAGE: OnceLock<PathBuf> = OnceLock::new();
    STORAGE.get_or_init(|| {
        std::env::var_os("PNPM_REGISTRY_STORAGE")
            .map(PathBuf::from)
            .or_else(|| {
                home::home_dir()
                    .map(|home| home.join(".cache").join("pnpm-registry").join("storage"))
            })
            .expect("locate runtime storage dir: set PNPM_REGISTRY_STORAGE or ensure $HOME is set")
    })
}
