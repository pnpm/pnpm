use super::{
    AsyncWriteExt, AtomicU64, ErrorKind, Ordering, Path, PathBuf, Result, fs, read_dir_if_present,
};

/// Per-process counter feeding [`unique_tmp_path`] so two concurrent
/// writes to the same path don't collide on the same temp filename.
/// Combined with the pid and random suffix, the rename is still atomic
/// on POSIX as long as src and dest sit in the same directory (they do).
pub(super) static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) const MAX_TEMP_CREATE_ATTEMPTS: usize = 16;

pub async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with_replace(path, bytes, true).await
}

/// Publishes a complete file without replacing an existing destination.
pub async fn write_atomic_new(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with_replace(path, bytes, false).await
}

pub(super) async fn write_atomic_with_replace(
    path: &Path,
    bytes: &[u8],
    replace: bool,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let (mut file, tmp) = create_tmp_file(path).await?;
    if let Err(err) = file.write_all(bytes).await {
        drop(file);
        let _ = fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    if let Err(err) = file.sync_all().await {
        drop(file);
        let _ = fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    drop(file);
    let committed =
        if replace { fs::rename(&tmp, path).await } else { fs::hard_link(&tmp, path).await };
    if let Err(err) = committed {
        let _ = fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    if !replace && let Err(err) = fs::remove_file(&tmp).await {
        tracing::warn!(?err, path = %tmp.display(), "atomic publication temp cleanup failed");
    }
    Ok(())
}

pub async fn remove_atomic_write_temps(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let Some(file_name) = path.file_name() else {
        return Ok(());
    };
    let mut prefix = file_name.to_os_string();
    prefix.push(".tmp.");
    let Some(mut entries) = read_dir_if_present(parent).await? else {
        return Ok(());
    };
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(suffix) = name.as_encoded_bytes().strip_prefix(prefix.as_encoded_bytes()) else {
            continue;
        };
        if !is_atomic_write_temp_suffix(suffix) {
            continue;
        }
        match fs::remove_file(entry.path()).await {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

pub(super) fn is_atomic_write_temp_suffix(suffix: &[u8]) -> bool {
    let mut parts = suffix.split(|byte| *byte == b'.');
    let (Some(pid), Some(counter), Some(random), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !pid.is_empty()
        && pid.iter().all(u8::is_ascii_digit)
        && !counter.is_empty()
        && counter.iter().all(u8::is_ascii_digit)
        && random.len() == 16
        && random.iter().all(u8::is_ascii_hexdigit)
}

pub(super) async fn create_tmp_file(base: &Path) -> Result<(fs::File, PathBuf)> {
    create_tmp_file_with(base, unique_tmp_path).await
}

pub(super) async fn create_tmp_file_with(
    base: &Path,
    mut next_path: impl FnMut(&Path) -> PathBuf,
) -> Result<(fs::File, PathBuf)> {
    let mut last_already_exists = None;
    for _ in 0..MAX_TEMP_CREATE_ATTEMPTS {
        let tmp_path = next_path(base);
        match fs::OpenOptions::new().read(true).write(true).create_new(true).open(&tmp_path).await {
            Ok(file) => return Ok((file, tmp_path)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                last_already_exists = Some(err);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(last_already_exists
        .unwrap_or_else(|| {
            std::io::Error::new(ErrorKind::AlreadyExists, "temporary path creation collided")
        })
        .into())
}

/// A unique sibling of `base` (`<base>.tmp.<pid>.<counter>.<random>`).
///
/// Keeping it in `base`'s directory keeps the eventual rename atomic on POSIX.
/// Shared with the S3 backend's staging path.
pub fn unique_tmp_path(base: &Path) -> PathBuf {
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut random = [0u8; 8];
    let random = match getrandom::fill(&mut random) {
        Ok(()) => u64::from_ne_bytes(random),
        Err(_) => 0,
    };
    let mut name = base.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(format!(".tmp.{pid}.{counter}.{random:016x}"));
    match base.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}
