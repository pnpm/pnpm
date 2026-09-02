//! Candidate identity: which package provides a bin, and the
//! fingerprint a trust approval is bound to.

use pnpm_crypto_hash::{create_hex_hash, create_hex_hash_bytes, create_hex_hash_from_file};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) const MAX_HASHED_BIN_SIZE: u64 = 1024 * 1024;

pub(super) struct Provider {
    pub(super) name: String,
    pub(super) package_dir: PathBuf,
    pub(super) manifest_hash: String,
}

pub(super) struct LocalBinIdentity {
    pub(super) provider: Provider,
    pub(super) fingerprint: String,
}

/// Resolve a target through aliases/workspace links, then read the nearest
/// package manifest. Package identity comes from the manifest rather than the
/// attacker-controlled alias or shim path.
pub(super) fn provider_of_target(target: &Path) -> Option<Provider> {
    let target = dunce::canonicalize(target).ok()?;
    let package_dir = package_dir_of_target(&target)?;
    let manifest = std::fs::read(package_dir.join("package.json")).ok()?;
    let parsed: Value = serde_json::from_slice(&manifest).ok()?;
    let name = parsed.get("name").and_then(Value::as_str)?.to_string();
    Some(Provider { name, package_dir, manifest_hash: create_hex_hash_bytes(&manifest) })
}

pub(super) fn package_dir_of_target(target: &Path) -> Option<PathBuf> {
    for dir in target.parent()?.ancestors() {
        if dir.join("package.json").is_file() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

pub(super) fn local_bin_identity(bin: &Path, name: &str) -> Option<LocalBinIdentity> {
    let metadata = std::fs::symlink_metadata(bin).ok()?;
    let (target, bin_hash) = if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(bin).ok()?;
        let hash = create_hex_hash(&format!("symlink\0{}", target.display()));
        (target, hash)
    } else {
        let script = bin.parent()?.join(name);
        let content = std::fs::read_to_string(&script).ok()?;
        let target = read_shim_target_from_content(&content)?;
        // The executed flavor can differ from the trailer-carrying sh
        // flavor (`tool.cmd` vs `tool` on Windows), so the fingerprint
        // binds both: replacing either file invalidates an approval.
        let executed_hash = if script == bin {
            String::new()
        } else {
            let executed_len = std::fs::metadata(bin).ok()?.len();
            small_file_hash(bin, executed_len)?
        };
        (target, create_hex_hash(&format!("script\0{content}\0{executed_hash}")))
    };
    let resolved = if target.is_absolute() { target } else { bin.parent()?.join(target) };
    let provider = provider_of_target(&resolved)?;
    let target = dunce::canonicalize(resolved).ok()?;
    let target_stat = file_identity(&target)?;
    let lockfile_hash = project_lockfile_hash(bin);
    let fingerprint = create_hex_hash(&format!(
        "bin\0{name}\0{}\0{}\0{}\0{}\0{}\0{bin_hash}\0{lockfile_hash}",
        provider.name,
        provider.package_dir.display(),
        provider.manifest_hash,
        target.display(),
        target_stat,
    ));
    Some(LocalBinIdentity { provider, fingerprint })
}

pub(super) fn project_lockfile_hash(path: &Path) -> String {
    path.ancestors()
        .find_map(|dir| create_hex_hash_from_file(&dir.join("pnpm-lock.yaml")).ok())
        .unwrap_or_else(|| "none".to_string())
}

pub(super) fn file_identity(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    #[cfg(unix)]
    let platform_identity = {
        use std::os::unix::fs::MetadataExt as _;
        format!("{}:{}", metadata.dev(), metadata.ino())
    };
    #[cfg(windows)]
    let platform_identity = windows_file_identity(path)?;
    #[cfg(not(any(unix, windows)))]
    let platform_identity = "0";
    let content_hash = small_file_hash(path, metadata.len()).unwrap_or_else(|| "large".to_string());
    Some(format!("{}:{modified_ns}:{platform_identity}:{content_hash}", metadata.len()))
}

pub(super) fn small_file_hash(path: &Path, expected_len: u64) -> Option<String> {
    use std::io::Read as _;

    if expected_len > MAX_HASHED_BIN_SIZE {
        return None;
    }
    let mut bytes = Vec::with_capacity(expected_len as usize);
    std::fs::File::open(path).ok()?.take(MAX_HASHED_BIN_SIZE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_HASHED_BIN_SIZE).then(|| create_hex_hash_bytes(&bytes))
}

#[cfg(windows)]
pub(super) fn windows_file_identity(path: &Path) -> Option<String> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = std::fs::File::open(path).ok()?;
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` owns a valid handle for this call and `info` points to
    // writable storage of the exact structure the API initializes.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0 {
        return None;
    }
    // SAFETY: a successful `GetFileInformationByHandle` initializes `info`.
    let info = unsafe { info.assume_init() };
    Some(format!("{}:{}:{}", info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
}

pub(super) fn read_shim_target_from_content(content: &str) -> Option<PathBuf> {
    content
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("# cmd-shim-target="))
        .map(PathBuf::from)
}
