use pipe_trait::Pipe;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

#[must_use]
pub fn workspace_root() -> &'static Path {
    static WORKSPACE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
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
        output.stdout
            .pipe(String::from_utf8)
            .expect("convert stdout to UTF-8")
            .trim_end()
            .pipe(Path::new)
            .parent()
            .expect("parent of root manifest")
            .to_path_buf()
    });
    WORKSPACE_ROOT.as_path()
}

/// Root the per-generation runtime storage directories live under.
///
/// The mock cannot point `pnpr` at the generated fixture storage itself.
/// `pnpr` writes proxy-mode cache entries (the ~2.3k unscoped npm packages
/// the benchmark lockfile pulls) into `--storage`, and those would be mixed
/// into the generated tree and lost whenever the fixtures are rebuilt. CI
/// also caches this root across runs
/// (`.github/workflows/pacquet-integrated-benchmark.yml`); without that,
/// cold-cache scenarios pay a full 2.3k-packument fetch from npmjs every
/// run.
///
/// `PNPM_REGISTRY_STORAGE` overrides the root, not one generation's
/// directory within it.
///
/// Defaults to `$HOME/.cache/pnpm-registry/storage`.
#[must_use]
pub fn runtime_storage_root() -> &'static Path {
    static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
        std::env::var_os("PNPM_REGISTRY_STORAGE")
            .map(PathBuf::from)
            .or_else(|| {
                home::home_dir()
                    .map(|home| {
                        home.join(".cache")
                            .join("pnpm-registry")
                            .join("storage")
                    })
            })
            .expect("locate runtime storage root: set PNPM_REGISTRY_STORAGE or ensure $HOME is set")
    });
    ROOT.as_path()
}
