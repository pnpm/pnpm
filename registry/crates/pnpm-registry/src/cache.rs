use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{RegistryError, Result};
use crate::package_name::PackageName;

const PACKUMENT_FILE: &str = "packument.json";
const TARBALLS_DIR: &str = "-";

/// Filesystem cache for packuments and tarballs. The layout is
/// verdaccio-shaped:
///
/// ```text
/// <root>/
///   <package>/
///     packument.json
///     -/
///       <package>-<version>.tgz
/// ```
///
/// For scoped packages the package directory is `<root>/@scope/<name>/`.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Read a cached packument if it exists and is newer than
    /// `now - ttl`.
    pub async fn read_fresh_packument(
        &self,
        name: &PackageName,
        ttl: Duration,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.packument_path(name);
        let metadata = match fs::metadata(&path).await {
            Ok(m) => m,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mtime = metadata.modified().map_err(RegistryError::Io)?;
        let age = SystemTime::now().duration_since(mtime).unwrap_or(Duration::ZERO);
        if age > ttl {
            return Ok(None);
        }
        Ok(Some(fs::read(&path).await?))
    }

    /// Read whatever packument is on disk, fresh or stale. Used as a
    /// fallback when the upstream is unreachable.
    pub async fn read_packument_any_age(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        let path = self.packument_path(name);
        match fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn write_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        let path = self.packument_path(name);
        write_atomic(&path, bytes).await
    }

    pub async fn read_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.tarball_path(name, filename);
        match fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn write_tarball(
        &self,
        name: &PackageName,
        filename: &str,
        bytes: &[u8],
    ) -> Result<()> {
        let path = self.tarball_path(name, filename);
        write_atomic(&path, bytes).await
    }

    fn package_dir(&self, name: &PackageName) -> PathBuf {
        self.root.join(name.as_str())
    }

    fn packument_path(&self, name: &PackageName) -> PathBuf {
        self.package_dir(name).join(PACKUMENT_FILE)
    }

    fn tarball_path(&self, name: &PackageName, filename: &str) -> PathBuf {
        self.package_dir(name).join(TARBALLS_DIR).join(filename)
    }
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&tmp, path).await?;
    Ok(())
}
