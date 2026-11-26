use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpz_nix_store::StorePath;
use std::{
    env,
    fs,
    io,
    os::unix::fs::PermissionsExt,
    path::{
        Path,
        PathBuf,
    },
    process,
};

const NATIVE_ROOT: &str = "/nix";

/// Overrides where the store is kept. The directory takes the place of `/nix`.
pub const STORE_ROOT_ENV: &str = "PNPZ_STORE_ROOT";

/// The directory that holds store paths, addressed by its parent: the
/// directory that stands for `/nix`.
///
/// A *native* store is `/nix` itself. Any other store has to be mounted at
/// `/nix` before its packages run, which [`crate::sandbox`] does on Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum StoreError {
    #[display("failed to prepare the store at {}: {source}", root.display())]
    #[diagnostic(code(ERR_PNPZ_STORE_UNAVAILABLE))]
    Unavailable { root: PathBuf, source: io::Error },
    #[display("pnpz found no home directory to keep its store in")]
    #[diagnostic(
        code(ERR_PNPZ_NO_HOME),
        help("set {STORE_ROOT_ENV} to a directory to use for the store")
    )]
    NoHome,
}

impl Store {
    /// Picks the store: `PNPZ_STORE_ROOT`, a writable `/nix`, or a directory
    /// in the user's data directory, in that order.
    pub fn discover() -> Result<Self, StoreError> {
        if let Some(root) = env::var_os(STORE_ROOT_ENV) {
            return Self::at(PathBuf::from(root));
        }
        if is_writable_directory(&Path::new(NATIVE_ROOT).join("store")) {
            return Self::at(PathBuf::from(NATIVE_ROOT));
        }
        let data_directory = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home::home_dir().map(|home| home.join(".local/share")))
            .ok_or(StoreError::NoHome)?;
        Self::at(data_directory.join("pnpz/nix"))
    }

    pub fn at(root: PathBuf) -> Result<Self, StoreError> {
        match fs::create_dir_all(root.join("store")) {
            Ok(()) => Ok(Self { root }),
            Err(source) => Err(StoreError::Unavailable { root, source }),
        }
    }

    /// The directory that stands for `/nix`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_native(&self) -> bool {
        self.root == Path::new(NATIVE_ROOT)
    }

    /// Where `path` is kept on disk, which differs from the path itself
    /// unless the store is native.
    pub fn location_of(&self, path: &StorePath) -> PathBuf {
        self.root.join("store").join(path.base_name())
    }

    pub fn contains(&self, path: &StorePath) -> bool {
        fs::symlink_metadata(self.location_of(path)).is_ok()
    }

    /// A path to unpack `path` at before [`Store::commit`] moves it into
    /// place.
    ///
    /// It is a sibling of the final location because a directory that is not
    /// writable, as every store directory is, can only be renamed within its
    /// parent.
    pub fn staging_location_of(&self, path: &StorePath) -> PathBuf {
        self.root
            .join("store")
            .join(format!(".pnpz-{}-{}", process::id(), path.base_name()))
    }

    /// Moves a fully unpacked and verified tree into the store.
    pub fn commit(&self, staging: &Path, path: &StorePath) -> io::Result<()> {
        match fs::rename(staging, self.location_of(path)) {
            // Another process installed the same path first. Its contents are
            // identical, because both were verified against the same hash.
            Err(_) if self.contains(path) => remove_tree(staging),
            result => result,
        }
    }
}

fn is_writable_directory(directory: &Path) -> bool {
    let probe = directory.join(format!(".pnpz-probe-{}", process::id()));
    fs::create_dir(&probe).and_then(|()| fs::remove_dir(&probe)).is_ok()
}

/// Deletes a tree whose directories may be read-only, as unpacked store paths
/// are.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        result => result?,
    };
    if !metadata.is_dir() {
        return fs::remove_file(path);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(path)? {
        remove_tree(&entry?.path())?;
    }
    fs::remove_dir(path)
}

#[cfg(test)]
mod tests;
